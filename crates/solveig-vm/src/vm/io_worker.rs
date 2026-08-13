//! Blocking host IO workers and scheduler-facing request/completion state.
//! Workers own no VM values or heap references; completions cross back as plain
//! Rust data and are converted only after the scheduler wakes the process.

use super::*;
use crate::handle::{
    FIRST_FILE_HANDLE, FileMode, HandleFailure, HandleId, HandleOutput, HandleRequest, HandleTable,
};
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};
use std::thread;
use std::time::Duration;

pub(super) mod filesystem;
pub(super) mod process;

use filesystem::{FilesystemFailure, FilesystemRequest};
use process::{ProcessFailure, ProcessOutput, ProcessRequest};

use super::socket::{SocketRequest, SocketSubsystem};

const IO_WORKER_COUNT: usize = 4;

pub(super) enum IoRequest {
    Print(String),
    ReadLine,
    ReadFile(PathBuf),
    ReadFileBytes(PathBuf),
    WriteFile {
        path: PathBuf,
        contents: String,
        append: bool,
    },
    WriteFileBytes {
        path: PathBuf,
        contents: Vec<u8>,
    },
    Filesystem(FilesystemRequest),
    Process(ProcessRequest),
    ResolveHost(String),
    Connect {
        host: String,
        port: i64,
    },
    Listen {
        port: i64,
    },
    Accept(SocketId),
    SendBytes {
        socket: SocketId,
        bytes: Vec<u8>,
    },
    RecvBytes {
        socket: SocketId,
        maximum: i64,
    },
    CloseSocket(SocketId),
    Handle(HandleRequest),
}

pub(super) enum IoResult {
    Unit,
    Text(String),
    Bytes(Vec<u8>),
    Socket(SocketId),
    Listener { socket: SocketId, port: u16 },
    Handle(HandleId),
    Bool(bool),
    Int(i64),
    TextList(Vec<String>),
    DirectoryChanged(PathBuf),
    Process(ProcessOutput),
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
    GetEnv(String),
    SetEnv(String),
    Filesystem(FilesystemFailure),
    Process(ProcessFailure),
    ResolveHost {
        host: String,
        source: io::Error,
    },
    Connect {
        host: String,
        port: i64,
        source: io::Error,
    },
    Listen {
        port: i64,
        source: io::Error,
    },
    Accept(io::Error),
    Send(io::Error),
    Recv(io::Error),
    CloseSocket(io::Error),
    Handle(HandleFailure),
}

struct IoWork {
    id: RequestId,
    request: IoRequest,
}

pub(super) struct IoCompletion {
    pub(super) id: RequestId,
    pub(super) result: IoResult,
}

#[derive(Clone, Copy)]
enum RequestOwner {
    Process(ProcessId),
    Cancelled,
}

pub(super) struct IoSubsystem {
    workers: Vec<mpsc::Sender<IoWork>>,
    sockets: SocketSubsystem,
    completions: mpsc::Receiver<IoCompletion>,
    requests: Vec<Option<RequestOwner>>,
    free_requests: Vec<u32>,
    next_worker: usize,
    handle_workers: Vec<Option<usize>>,
    args: Vec<String>,
    cwd: PathBuf,
    environment: std::collections::BTreeMap<String, String>,
    program_name: String,
}

impl IoSubsystem {
    pub(super) fn inactive() -> Self {
        let (_completion_tx, completions) = mpsc::channel();
        Self {
            workers: Vec::new(),
            sockets: SocketSubsystem::inactive(),
            completions,
            requests: Vec::new(),
            free_requests: Vec::new(),
            next_worker: 0,
            handle_workers: vec![None; FIRST_FILE_HANDLE as usize],
            args: Vec::new(),
            cwd: PathBuf::new(),
            environment: std::collections::BTreeMap::new(),
            program_name: String::new(),
        }
    }

    pub(super) fn start<'scope>(
        scope: &'scope thread::Scope<'scope, '_>,
        stdin: &'scope Mutex<&mut (dyn BufRead + Send)>,
        stdout: &'scope Mutex<&mut (dyn Write + Send)>,
        stderr: &'scope Mutex<&mut (dyn Write + Send)>,
        terminal_outputs: (bool, bool),
        args: &[String],
        cwd: &Path,
    ) -> Result<Self, RuntimeError> {
        let cwd = fs::canonicalize(cwd).map_err(|error| {
            RuntimeError::builtin(format!(
                "failed to resolve the runtime working directory: {error}"
            ))
        })?;
        let program_name = cwd
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("solveig")
            .to_string();
        let (completion_tx, completions) = mpsc::channel();
        let sockets = SocketSubsystem::start(scope, completion_tx.clone()).map_err(|error| {
            RuntimeError::builtin(format!("failed to start socket event loop: {error}"))
        })?;
        let mut workers = Vec::with_capacity(IO_WORKER_COUNT);

        for worker_index in 0..IO_WORKER_COUNT {
            let (work_tx, work_rx) = mpsc::channel();
            let completion_tx = completion_tx.clone();
            let handle = thread::Builder::new()
                .name(format!("solveig-io-{worker_index}"))
                .spawn_scoped(scope, move || {
                    worker_loop(
                        worker_index,
                        &work_rx,
                        &completion_tx,
                        stdin,
                        stdout,
                        stderr,
                        terminal_outputs,
                    );
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
            sockets,
            completions,
            requests: Vec::new(),
            free_requests: Vec::new(),
            next_worker: 0,
            handle_workers: vec![Some(0), Some(1), Some(2)],
            args: args.to_vec(),
            cwd,
            environment: std::env::vars_os()
                .filter_map(|(name, value)| {
                    let name = name.into_string().ok()?;
                    validate_environment_name(&name).ok()?;
                    Some((name, value.into_string().ok()?))
                })
                .collect(),
            program_name,
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
            // operations on the shared terminal streams. File and resolver
            // requests can still run concurrently on the rest of the pool.
            IoRequest::Print(_)
            | IoRequest::ReadLine
            | IoRequest::Connect { .. }
            | IoRequest::Listen { .. }
            | IoRequest::Accept(_)
            | IoRequest::SendBytes { .. }
            | IoRequest::RecvBytes { .. }
            | IoRequest::CloseSocket(_) => 0,
            IoRequest::Handle(request) => {
                let handle = request.handle();
                self.handle_workers
                    .get(handle.0 as usize)
                    .and_then(|worker| *worker)
                    .ok_or_else(|| {
                        RuntimeError::builtin(format!("unknown Handle identity {}", handle.0))
                    })?
            }
            IoRequest::ReadFile(_)
            | IoRequest::ReadFileBytes(_)
            | IoRequest::WriteFile { .. }
            | IoRequest::WriteFileBytes { .. }
            | IoRequest::Filesystem(_)
            | IoRequest::Process(_)
            | IoRequest::ResolveHost(_) => {
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
        self.next_worker = self.next_worker.wrapping_add(1);

        let issued = match request {
            IoRequest::Connect { host, port } => self
                .sockets
                .issue(id, SocketRequest::Connect { host, port }),
            IoRequest::Listen { port } => self.sockets.issue(id, SocketRequest::Listen { port }),
            IoRequest::Accept(socket) => self.sockets.issue(id, SocketRequest::Accept(socket)),
            IoRequest::SendBytes { socket, bytes } => self
                .sockets
                .issue(id, SocketRequest::Send { socket, bytes }),
            IoRequest::RecvBytes { socket, maximum } => self
                .sockets
                .issue(id, SocketRequest::Recv { socket, maximum }),
            IoRequest::CloseSocket(socket) => self.sockets.issue(id, SocketRequest::Close(socket)),
            request => {
                let Some(worker) = self.workers.get(worker_index) else {
                    solveig_base::ice!("IO request routing selected an absent worker");
                };
                worker
                    .send(IoWork { id, request })
                    .map_err(|_disconnected| {
                        RuntimeError::builtin("IO worker pool stopped unexpectedly".to_string())
                    })
            }
        };

        if let Err(error) = issued {
            let Some(slot) = self.requests.get_mut(id.0 as usize) else {
                solveig_base::ice!("failed IO request vanished from the request table");
            };
            *slot = None;
            self.free_requests.push(id.0);
            return Err(error);
        }
        Ok(id)
    }

    pub(super) fn issue_open_file(
        &mut self,
        process: ProcessId,
        path: PathBuf,
        mode: FileMode,
    ) -> Result<RequestId, RuntimeError> {
        let raw = u32::try_from(self.handle_workers.len()).map_err(|_too_many_handles| {
            RuntimeError::builtin("handle table exhausted".to_string())
        })?;
        let worker = self.next_worker % self.workers.len();
        self.handle_workers.push(Some(worker));
        self.issue(
            process,
            IoRequest::Handle(HandleRequest::OpenFile {
                handle: HandleId(raw),
                path,
                mode,
            }),
        )
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

    pub(super) fn cancel(&mut self, id: RequestId, process: ProcessId) -> Result<(), RuntimeError> {
        let Some(slot) = self.requests.get_mut(id.0 as usize) else {
            solveig_base::ice!("cancelled IO request was outside the request table");
        };
        match slot {
            Some(RequestOwner::Process(owner)) if *owner == process => {
                *slot = Some(RequestOwner::Cancelled);
                self.sockets.cancel(id)?;
            }
            Some(RequestOwner::Process(_)) | Some(RequestOwner::Cancelled) | None => {
                solveig_base::ice!("cancelled IO request did not belong to the blocked process")
            }
        }
        Ok(())
    }

    pub(super) fn has_wakeable_request(&self) -> bool {
        self.requests
            .iter()
            .any(|request| matches!(request, Some(RequestOwner::Process(_))))
    }

    pub(super) fn args(&self) -> &[String] {
        &self.args
    }

    pub(super) fn get_env(&self, name: &str) -> Result<Option<&str>, IoFailure> {
        validate_environment_name(name).map_err(IoFailure::GetEnv)?;
        Ok(self.environment.get(name).map(String::as_str))
    }

    pub(super) fn set_env(&mut self, name: String, value: String) -> Result<(), IoFailure> {
        validate_environment_name(&name).map_err(IoFailure::SetEnv)?;
        if value.contains('\0') {
            return Err(IoFailure::SetEnv(
                "environment values cannot contain NUL".to_string(),
            ));
        }
        self.environment.insert(name, value);
        Ok(())
    }

    pub(super) fn environment(&self) -> &std::collections::BTreeMap<String, String> {
        &self.environment
    }

    pub(super) fn program_name(&self) -> &str {
        &self.program_name
    }

    pub(super) fn current_directory(&self) -> &Path {
        &self.cwd
    }

    pub(super) fn set_current_directory(&mut self, path: PathBuf) {
        self.cwd = path;
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
    worker_index: usize,
    work: &mpsc::Receiver<IoWork>,
    completions: &mpsc::Sender<IoCompletion>,
    stdin: &Mutex<&mut (dyn BufRead + Send)>,
    stdout: &Mutex<&mut (dyn Write + Send)>,
    stderr: &Mutex<&mut (dyn Write + Send)>,
    terminal_outputs: (bool, bool),
) {
    let mut handles = HandleTable::for_worker(worker_index, terminal_outputs);
    while let Ok(work) = work.recv() {
        let result = perform_request(work.request, stdin, stdout, stderr, &mut handles);
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
    handles.shutdown(stdout, stderr);
}

fn perform_request(
    request: IoRequest,
    stdin: &Mutex<&mut (dyn BufRead + Send)>,
    stdout: &Mutex<&mut (dyn Write + Send)>,
    stderr: &Mutex<&mut (dyn Write + Send)>,
    handles: &mut HandleTable,
) -> IoResult {
    match request {
        IoRequest::Print(text) => write_stdout(stdout, &text),
        IoRequest::ReadLine => perform_read_line(stdin),
        IoRequest::ReadFile(path) => match fs::read_to_string(&path) {
            Ok(contents) => IoResult::Text(contents),
            Err(source) => IoResult::Failed(IoFailure::ReadFile { path, source }),
        },
        IoRequest::ReadFileBytes(path) => match fs::read(&path) {
            Ok(contents) => IoResult::Bytes(contents),
            Err(source) => IoResult::Failed(IoFailure::ReadFile { path, source }),
        },
        IoRequest::WriteFile {
            path,
            contents,
            append,
        } => perform_write_file(path, &contents, append),
        IoRequest::WriteFileBytes { path, contents } => match fs::write(&path, contents) {
            Ok(()) => IoResult::Unit,
            Err(source) => IoResult::Failed(IoFailure::WriteFile {
                path,
                append: false,
                source,
            }),
        },
        IoRequest::Filesystem(request) => filesystem::perform(request),
        IoRequest::Process(request) => process::perform(request),
        IoRequest::ResolveHost(host) => perform_resolve_host(host),
        IoRequest::Handle(request) => match handles.perform(request, stdin, stdout, stderr) {
            Ok(HandleOutput::Unit) => IoResult::Unit,
            Ok(HandleOutput::Opened(handle)) => IoResult::Handle(handle),
            Ok(HandleOutput::Text(text)) => IoResult::Text(text),
            Ok(HandleOutput::Bytes(bytes)) => IoResult::Bytes(bytes),
            Ok(HandleOutput::Bool(value)) => IoResult::Bool(value),
            Err(failure) => IoResult::Failed(IoFailure::Handle(failure)),
        },
        IoRequest::Connect { .. }
        | IoRequest::Listen { .. }
        | IoRequest::Accept(_)
        | IoRequest::SendBytes { .. }
        | IoRequest::RecvBytes { .. }
        | IoRequest::CloseSocket(_) => {
            solveig_base::ice!("socket request reached the blocking worker pool")
        }
    }
}

fn validate_environment_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        Err("environment variable names cannot be empty".to_string())
    } else if name.contains(['=', '\0']) {
        Err("environment variable names cannot contain '=' or NUL".to_string())
    } else {
        Ok(())
    }
}

fn perform_resolve_host(host: String) -> IoResult {
    let addresses = match (host.as_str(), 0).to_socket_addrs() {
        Ok(addresses) => addresses,
        Err(source) => return IoResult::Failed(IoFailure::ResolveHost { host, source }),
    };
    match addresses.into_iter().find(|address| address.is_ipv4()) {
        Some(address) => IoResult::Text(address.ip().to_string()),
        None => IoResult::Failed(IoFailure::ResolveHost {
            host,
            source: io::Error::new(io::ErrorKind::AddrNotAvailable, "host has no IPv4 address"),
        }),
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
