//! Blocking host IO workers and scheduler-facing request/completion state.
//! Workers own no VM values or heap references; completions cross back as plain
//! Rust data and are converted only after the scheduler wakes the process.

use super::*;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};
use std::thread;
use std::time::Duration;

const IO_WORKER_COUNT: usize = 4;

pub(super) enum IoRequest {
    Print(String),
    ReadLine,
    ReadFile(PathBuf),
    WriteFile {
        path: PathBuf,
        contents: String,
        append: bool,
    },
}

pub(super) enum IoResult {
    Unit,
    Text(String),
    Failed(IoFailure),
}

pub(super) enum IoFailure {
    Print(io::Error),
    ReadLine(io::Error),
    ReadFile {
        path: PathBuf,
        source: io::Error,
    },
    WriteFile {
        path: PathBuf,
        append: bool,
        source: io::Error,
    },
}

struct IoWork {
    id: RequestId,
    request: IoRequest,
}

struct IoCompletion {
    id: RequestId,
    result: IoResult,
}

#[derive(Clone, Copy)]
enum RequestOwner {
    Process(ProcessId),
    Cancelled,
}

pub(super) struct IoSubsystem {
    workers: Vec<mpsc::Sender<IoWork>>,
    completions: mpsc::Receiver<IoCompletion>,
    requests: Vec<Option<RequestOwner>>,
    free_requests: Vec<u32>,
    next_worker: usize,
    args: Vec<String>,
    cwd: PathBuf,
}

impl IoSubsystem {
    pub(super) fn inactive() -> Self {
        let (_completion_tx, completions) = mpsc::channel();
        Self {
            workers: Vec::new(),
            completions,
            requests: Vec::new(),
            free_requests: Vec::new(),
            next_worker: 0,
            args: Vec::new(),
            cwd: PathBuf::new(),
        }
    }

    pub(super) fn start<'scope>(
        scope: &'scope thread::Scope<'scope, '_>,
        stdin: &'scope Mutex<&mut (dyn BufRead + Send)>,
        stdout: &'scope Mutex<&mut (dyn Write + Send)>,
        args: &[String],
        cwd: &Path,
    ) -> Result<Self, RuntimeError> {
        let (completion_tx, completions) = mpsc::channel();
        let mut workers = Vec::with_capacity(IO_WORKER_COUNT);

        for worker_index in 0..IO_WORKER_COUNT {
            let (work_tx, work_rx) = mpsc::channel();
            let completion_tx = completion_tx.clone();
            let handle = thread::Builder::new()
                .name(format!("solveig-io-{worker_index}"))
                .spawn_scoped(scope, move || {
                    worker_loop(&work_rx, &completion_tx, stdin, stdout);
                })
                .map_err(|error| {
                    RuntimeError::builtin(format!("failed to start IO worker: {error}"))
                })?;
            drop(handle);
            workers.push(work_tx);
        }
        drop(completion_tx);

        Ok(Self {
            workers,
            completions,
            requests: Vec::new(),
            free_requests: Vec::new(),
            next_worker: 0,
            args: args.to_vec(),
            cwd: cwd.to_path_buf(),
        })
    }

    pub(super) fn issue(
        &mut self,
        process: ProcessId,
        request: IoRequest,
    ) -> Result<RequestId, RuntimeError> {
        if self.workers.is_empty() {
            return Err(RuntimeError::builtin(
                "IO request issued without an active worker pool".to_string(),
            ));
        }
        let worker_index = match &request {
            // A single FIFO worker preserves the program-visible order of
            // operations on the shared terminal streams. File requests can
            // still run concurrently on the rest of the pool.
            IoRequest::Print(_) | IoRequest::ReadLine => 0,
            IoRequest::ReadFile(_) | IoRequest::WriteFile { .. } => {
                1 + self.next_worker % (self.workers.len().saturating_sub(1))
            }
        };
        let id = if let Some(raw) = self.free_requests.pop() {
            let Some(slot) = self.requests.get_mut(raw as usize) else {
                solveig_base::ice!("free IO request id was outside the request table");
            };
            if slot.replace(RequestOwner::Process(process)).is_some() {
                solveig_base::ice!("free IO request id was still in use");
            }
            RequestId(raw)
        } else {
            let raw = u32::try_from(self.requests.len()).map_err(|_too_many_requests| {
                RuntimeError::builtin("IO request table exhausted".to_string())
            })?;
            self.requests.push(Some(RequestOwner::Process(process)));
            RequestId(raw)
        };
        let Some(worker) = self.workers.get(worker_index) else {
            solveig_base::ice!("IO request routing selected an absent worker");
        };
        self.next_worker = self.next_worker.wrapping_add(1);

        if worker.send(IoWork { id, request }).is_err() {
            let Some(slot) = self.requests.get_mut(id.0 as usize) else {
                solveig_base::ice!("failed IO request vanished from the request table");
            };
            *slot = None;
            self.free_requests.push(id.0);
            return Err(RuntimeError::builtin(
                "IO worker pool stopped unexpectedly".to_string(),
            ));
        }
        Ok(id)
    }

    pub(super) fn try_completion(
        &mut self,
    ) -> Result<Option<(RequestId, ProcessId, IoResult)>, RuntimeError> {
        loop {
            match self.completions.try_recv() {
                Ok(completion) => {
                    if let Some(completion) = self.finish_completion(completion)? {
                        return Ok(Some(completion));
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return Ok(None),
                Err(mpsc::TryRecvError::Disconnected) if !self.has_wakeable_request() => {
                    return Ok(None);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(RuntimeError::builtin(
                        "IO completion queue stopped with pending requests".to_string(),
                    ));
                }
            }
        }
    }

    pub(super) fn wait_completion(
        &mut self,
    ) -> Result<Option<(RequestId, ProcessId, IoResult)>, RuntimeError> {
        let completion = self.completions.recv().map_err(|_disconnected| {
            RuntimeError::builtin("IO completion queue stopped with pending requests".to_string())
        })?;
        self.finish_completion(completion)
    }

    pub(super) fn wait_completion_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<(RequestId, ProcessId, IoResult)>, RuntimeError> {
        match self.completions.recv_timeout(timeout) {
            Ok(completion) => self.finish_completion(completion),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) if !self.has_wakeable_request() => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(RuntimeError::builtin(
                "IO completion queue stopped with pending requests".to_string(),
            )),
        }
    }

    pub(super) fn cancel(&mut self, id: RequestId, process: ProcessId) {
        let Some(slot) = self.requests.get_mut(id.0 as usize) else {
            solveig_base::ice!("cancelled IO request was outside the request table");
        };
        match slot {
            Some(RequestOwner::Process(owner)) if *owner == process => {
                *slot = Some(RequestOwner::Cancelled);
            }
            Some(RequestOwner::Process(_)) | Some(RequestOwner::Cancelled) | None => {
                solveig_base::ice!("cancelled IO request did not belong to the blocked process")
            }
        }
    }

    pub(super) fn has_wakeable_request(&self) -> bool {
        self.requests
            .iter()
            .any(|request| matches!(request, Some(RequestOwner::Process(_))))
    }

    pub(super) fn args(&self) -> &[String] {
        &self.args
    }

    pub(super) fn resolve_path(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        }
    }

    fn finish_completion(
        &mut self,
        completion: IoCompletion,
    ) -> Result<Option<(RequestId, ProcessId, IoResult)>, RuntimeError> {
        let Some(slot) = self.requests.get_mut(completion.id.0 as usize) else {
            solveig_base::ice!("IO completion referenced an unknown request");
        };
        let Some(owner) = slot.take() else {
            solveig_base::ice!("IO request completed more than once");
        };
        self.free_requests.push(completion.id.0);
        Ok(match owner {
            RequestOwner::Process(process) => Some((completion.id, process, completion.result)),
            RequestOwner::Cancelled => None,
        })
    }
}

fn worker_loop(
    work: &mpsc::Receiver<IoWork>,
    completions: &mpsc::Sender<IoCompletion>,
    stdin: &Mutex<&mut (dyn BufRead + Send)>,
    stdout: &Mutex<&mut (dyn Write + Send)>,
) {
    while let Ok(work) = work.recv() {
        let result = perform_request(work.request, stdin, stdout);
        if completions
            .send(IoCompletion {
                id: work.id,
                result,
            })
            .is_err()
        {
            return;
        }
    }
}

fn perform_request(
    request: IoRequest,
    stdin: &Mutex<&mut (dyn BufRead + Send)>,
    stdout: &Mutex<&mut (dyn Write + Send)>,
) -> IoResult {
    match request {
        IoRequest::Print(text) => write_stdout(stdout, &text),
        IoRequest::ReadLine => perform_read_line(stdin),
        IoRequest::ReadFile(path) => match fs::read_to_string(&path) {
            Ok(contents) => IoResult::Text(contents),
            Err(source) => IoResult::Failed(IoFailure::ReadFile { path, source }),
        },
        IoRequest::WriteFile {
            path,
            contents,
            append,
        } => perform_write_file(path, &contents, append),
    }
}

fn write_stdout(stdout: &Mutex<&mut (dyn Write + Send)>, text: &str) -> IoResult {
    let mut stdout = match stdout.lock() {
        Ok(stdout) => stdout,
        Err(_) => {
            return IoResult::Failed(IoFailure::Print(io::Error::other(
                "stdout lock was poisoned",
            )));
        }
    };
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => IoResult::Unit,
        Err(error) => IoResult::Failed(IoFailure::Print(error)),
    }
}

fn perform_read_line(stdin: &Mutex<&mut (dyn BufRead + Send)>) -> IoResult {
    let mut stdin = match stdin.lock() {
        Ok(stdin) => stdin,
        Err(_) => {
            return IoResult::Failed(IoFailure::ReadLine(io::Error::other(
                "stdin lock was poisoned",
            )));
        }
    };
    let mut line = String::new();
    match stdin.read_line(&mut line) {
        Ok(0) => IoResult::Text(String::new()),
        Ok(_) => {
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            IoResult::Text(line)
        }
        Err(error) => IoResult::Failed(IoFailure::ReadLine(error)),
    }
}

fn perform_write_file(path: PathBuf, contents: &str, append: bool) -> IoResult {
    let result = if append {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| file.write_all(contents.as_bytes()))
    } else {
        fs::write(&path, contents)
    };
    match result {
        Ok(()) => IoResult::Unit,
        Err(source) => IoResult::Failed(IoFailure::WriteFile {
            path,
            append,
            source,
        }),
    }
}
