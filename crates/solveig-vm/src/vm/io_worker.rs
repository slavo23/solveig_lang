//! Blocking host IO workers and scheduler-facing request/completion state.
//! Workers own no VM values or heap references; completions cross back as plain
//! Rust data and are converted only after the scheduler wakes the process.

use super::http::{HttpFailure, HttpRequest, HttpResponse};
use super::*;
use crate::handle::{
    FIRST_FILE_HANDLE, FileMode, HandleFailure, HandleId, HandleOutput, HandleRequest, HandleTable,
};
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};
use std::thread;
use std::time::Duration;

pub(super) mod filesystem;
pub(super) mod process;
pub(super) mod streaming_process;

use filesystem::{FilesystemFailure, FilesystemRequest};
use process::{ProcessFailure, ProcessOutput, ProcessRequest, ProcessSignal};
use streaming_process::StreamingProcess;

use super::socket::{SocketRequest, SocketSubsystem};

// Worker zero preserves terminal ordering; four general workers provide the
// bounded host concurrency used by std.http and other blocking operations.
const IO_WORKER_COUNT: usize = 5;

pub(super) enum IoRequest {
    Print(String),
    ReadLine,
    ReadFile {
        path: PathBuf,
        maximum: Option<i64>,
    },
    ReadFileBytes {
        path: PathBuf,
        maximum: Option<i64>,
    },
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
    StartProcess(ProcessRequest),
    ReadProcessStdout {
        process: ChildProcessId,
        maximum: i64,
    },
    ReadProcessStderr {
        process: ChildProcessId,
        maximum: i64,
    },
    WriteProcessStdin {
        process: ChildProcessId,
        bytes: Vec<u8>,
    },
    CloseProcessStdin(ChildProcessId),
    WaitProcess(ChildProcessId),
    TerminateProcess(ChildProcessId),
    CloseProcess(ChildProcessId),
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
    Http(HttpRequest),
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
    StreamingProcess(StreamingProcess),
    Http(HttpResponse),
    Failed(IoFailure),
}

pub(super) enum IoFailure {
    Print(io::Error),
    ReadLine(io::Error),
    ReadFile {
        path: PathBuf,
        source: io::Error,
    },
    ReadFileLimited {
        path: PathBuf,
        source: io::Error,
    },
    ReadFileTooLarge {
        path: PathBuf,
        maximum: i64,
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
    ProcessStream {
        operation: &'static str,
        source: io::Error,
    },
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
    Http(HttpFailure),
}

enum IoWork {
    Request {
        id: RequestId,
        request: IoRequest,
    },
    Process {
        id: RequestId,
        request: ProcessRequest,
        signals: mpsc::Receiver<ProcessSignal>,
        signal_tx: mpsc::Sender<ProcessSignal>,
    },
    StartProcess {
        id: RequestId,
        request: ProcessRequest,
        signals: mpsc::Receiver<ProcessSignal>,
    },
}

enum IoCancellation {
    StrictProcess(mpsc::Sender<ProcessSignal>),
    StreamingProcess(ChildProcessId),
}

struct RequestSlot {
    owner: Option<RequestOwner>,
    cancellation: Option<IoCancellation>,
    closing_process: Option<StreamingProcess>,
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
    requests: Vec<RequestSlot>,
    streaming_processes: Vec<Option<StreamingProcess>>,
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
            streaming_processes: Vec::new(),
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
            streaming_processes: Vec::new(),
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
            IoRequest::ReadFile { .. }
            | IoRequest::ReadFileBytes { .. }
            | IoRequest::WriteFile { .. }
            | IoRequest::WriteFileBytes { .. }
            | IoRequest::Filesystem(_)
            | IoRequest::Process(_)
            | IoRequest::StartProcess(_)
            | IoRequest::ReadProcessStdout { .. }
            | IoRequest::ReadProcessStderr { .. }
            | IoRequest::WriteProcessStdin { .. }
            | IoRequest::CloseProcessStdin(_)
            | IoRequest::WaitProcess(_)
            | IoRequest::TerminateProcess(_)
            | IoRequest::CloseProcess(_)
            | IoRequest::ResolveHost(_)
            | IoRequest::Http(_) => 1 + self.next_worker % (self.workers.len().saturating_sub(1)),
        };
        let id = if let Some(raw) = self.free_requests.pop() {
            let Some(slot) = self.requests.get_mut(raw as usize) else {
                solveig_base::ice!("free IO request id was outside the request table");
            };
            if slot.owner.replace(RequestOwner::Process(process)).is_some() {
                solveig_base::ice!("free IO request id was still in use");
            }
            if slot.cancellation.is_some() {
                solveig_base::ice!("free IO request retained process cancellation state");
            }
            if slot.closing_process.is_some() {
                solveig_base::ice!("free IO request retained a closing process");
            }
            RequestId(raw)
        } else {
            let raw = u32::try_from(self.requests.len()).map_err(|_too_many_requests| {
                RuntimeError::builtin("IO request table exhausted".to_string())
            })?;
            self.requests.push(RequestSlot {
                owner: Some(RequestOwner::Process(process)),
                cancellation: None,
                closing_process: None,
            });
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
            IoRequest::Process(request) => {
                let (signal_tx, signals) = mpsc::channel();
                let Some(slot) = self.requests.get_mut(id.0 as usize) else {
                    solveig_base::ice!("process request had no request-table slot");
                };
                slot.cancellation = Some(IoCancellation::StrictProcess(signal_tx.clone()));
                let Some(worker) = self.workers.get(worker_index) else {
                    solveig_base::ice!("IO request routing selected an absent worker");
                };
                worker
                    .send(IoWork::Process {
                        id,
                        request,
                        signals,
                        signal_tx,
                    })
                    .map_err(|_disconnected| {
                        RuntimeError::builtin("IO worker pool stopped unexpectedly".to_string())
                    })
            }
            IoRequest::StartProcess(request) => {
                let (signal_tx, signals) = mpsc::channel();
                let Some(slot) = self.requests.get_mut(id.0 as usize) else {
                    solveig_base::ice!("process-start request had no request-table slot");
                };
                slot.cancellation = Some(IoCancellation::StrictProcess(signal_tx));
                let Some(worker) = self.workers.get(worker_index) else {
                    solveig_base::ice!("IO request routing selected an absent worker");
                };
                worker
                    .send(IoWork::StartProcess {
                        id,
                        request,
                        signals,
                    })
                    .map_err(|_disconnected| {
                        RuntimeError::builtin("IO worker pool stopped unexpectedly".to_string())
                    })
            }
            IoRequest::ReadProcessStdout { process, maximum } => {
                self.issue_streaming_operation(id, process, |resource| {
                    resource.read_stdout(id, maximum)
                })
            }
            IoRequest::ReadProcessStderr { process, maximum } => {
                self.issue_streaming_operation(id, process, |resource| {
                    resource.read_stderr(id, maximum)
                })
            }
            IoRequest::WriteProcessStdin { process, bytes } => {
                self.issue_streaming_operation(id, process, move |resource| {
                    resource.write_stdin(id, bytes)
                })
            }
            IoRequest::CloseProcessStdin(process) => {
                self.issue_streaming_operation(id, process, |resource| resource.close_stdin(id))
            }
            IoRequest::WaitProcess(process) => {
                self.issue_streaming_operation(id, process, |resource| resource.wait(id))
            }
            IoRequest::TerminateProcess(process) => {
                self.issue_streaming_operation(id, process, |resource| resource.terminate(id))
            }
            IoRequest::CloseProcess(process) => self.issue_close_process(id, process),
            request => {
                let Some(worker) = self.workers.get(worker_index) else {
                    solveig_base::ice!("IO request routing selected an absent worker");
                };
                worker
                    .send(IoWork::Request { id, request })
                    .map_err(|_disconnected| {
                        RuntimeError::builtin("IO worker pool stopped unexpectedly".to_string())
                    })
            }
        };

        if let Err(error) = issued {
            let Some(slot) = self.requests.get_mut(id.0 as usize) else {
                solveig_base::ice!("failed IO request vanished from the request table");
            };
            slot.owner = None;
            slot.cancellation = None;
            if let Some(process) = slot.closing_process.take() {
                process.shutdown_and_join();
            }
            self.free_requests.push(id.0);
            return Err(error);
        }
        Ok(id)
    }

    fn issue_streaming_operation(
        &mut self,
        request: RequestId,
        process: ChildProcessId,
        operation: impl FnOnce(&mut StreamingProcess) -> io::Result<()>,
    ) -> Result<(), RuntimeError> {
        operation(self.streaming_process_mut(process)?)
            .map_err(|error| streaming_issue_error(&error))?;
        self.track_streaming_cancellation(request, process);
        Ok(())
    }

    fn issue_close_process(
        &mut self,
        request: RequestId,
        process: ChildProcessId,
    ) -> Result<(), RuntimeError> {
        let Some(slot) = self.streaming_processes.get_mut(process.0 as usize) else {
            return Err(unknown_streaming_process(process));
        };
        let Some(resource) = slot.take() else {
            return Err(unknown_streaming_process(process));
        };
        if let Err(error) = resource.close(request) {
            resource.shutdown_and_join();
            return Err(streaming_issue_error(&error));
        }
        let Some(slot) = self.requests.get_mut(request.0 as usize) else {
            solveig_base::ice!("close-process request had no request-table slot");
        };
        slot.closing_process = Some(resource);
        Ok(())
    }

    fn track_streaming_cancellation(&mut self, request: RequestId, process: ChildProcessId) {
        let Some(slot) = self.requests.get_mut(request.0 as usize) else {
            solveig_base::ice!("streaming-process request had no request-table slot");
        };
        slot.cancellation = Some(IoCancellation::StreamingProcess(process));
    }

    fn streaming_process_mut(
        &mut self,
        process: ChildProcessId,
    ) -> Result<&mut StreamingProcess, RuntimeError> {
        self.streaming_processes
            .get_mut(process.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| unknown_streaming_process(process))
    }

    pub(super) fn register_streaming_process(
        &mut self,
        process: StreamingProcess,
    ) -> Result<u32, RuntimeError> {
        let raw = u32::try_from(self.streaming_processes.len()).map_err(|_too_many_processes| {
            RuntimeError::builtin("child-process table exhausted".to_string())
        })?;
        self.streaming_processes.push(Some(process));
        Ok(raw)
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
        let cancellation = match slot.owner {
            Some(RequestOwner::Process(owner)) if owner == process => {
                slot.owner = Some(RequestOwner::Cancelled);
                if let Some(process) = slot.closing_process.as_ref() {
                    process.cancel();
                }
                slot.cancellation.take()
            }
            Some(RequestOwner::Process(_)) | Some(RequestOwner::Cancelled) | None => {
                solveig_base::ice!("cancelled IO request did not belong to the blocked process")
            }
        };
        if let Some(cancellation) = cancellation {
            match cancellation {
                IoCancellation::StrictProcess(cancellation) => {
                    drop(cancellation.send(ProcessSignal::Cancel));
                }
                IoCancellation::StreamingProcess(process) => {
                    if let Some(process) = self
                        .streaming_processes
                        .get(process.0 as usize)
                        .and_then(Option::as_ref)
                    {
                        process.cancel();
                    }
                }
            }
        }
        self.sockets.cancel(id)?;
        Ok(())
    }

    pub(super) fn has_wakeable_request(&self) -> bool {
        self.requests
            .iter()
            .any(|request| matches!(request.owner, Some(RequestOwner::Process(_))))
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
        let Some(owner) = slot.owner.take() else {
            solveig_base::ice!("IO request completed more than once");
        };
        slot.cancellation = None;
        if let Some(process) = slot.closing_process.take() {
            process.join()?;
        }
        self.free_requests.push(completion.id.0);
        Ok(match (owner, completion.result) {
            (RequestOwner::Process(process), result) => Some((completion.id, process, result)),
            (RequestOwner::Cancelled, IoResult::StreamingProcess(process)) => {
                process.shutdown_and_join();
                None
            }
            (RequestOwner::Cancelled, _result) => None,
        })
    }
}

impl Drop for IoSubsystem {
    fn drop(&mut self) {
        for slot in &mut self.requests {
            if let Some(IoCancellation::StrictProcess(cancellation)) = slot.cancellation.take() {
                drop(cancellation.send(ProcessSignal::Cancel));
            }
            if let Some(process) = slot.closing_process.take() {
                process.shutdown_and_join();
            }
        }
        for process in self.streaming_processes.drain(..).flatten() {
            process.shutdown_and_join();
        }
    }
}

fn unknown_streaming_process(process: ChildProcessId) -> RuntimeError {
    RuntimeError::builtin(format!("unknown ChildProcess identity {}", process.0))
}

fn streaming_issue_error(error: &io::Error) -> RuntimeError {
    RuntimeError::builtin(format!("failed to issue child-process IO: {error}"))
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
    let http_agent = super::http::agent();
    while let Ok(work) = work.recv() {
        let (id, result) = match work {
            IoWork::Request { id, request } => (
                id,
                perform_request(request, stdin, stdout, stderr, &mut handles, &http_agent),
            ),
            IoWork::Process {
                id,
                request,
                signals,
                signal_tx,
            } => (id, process::perform(request, &signals, signal_tx)),
            IoWork::StartProcess {
                id,
                request,
                signals,
            } => {
                let result = match streaming_process::start(request, completions.clone(), &signals)
                {
                    Ok(process) => IoResult::StreamingProcess(process),
                    Err(failure) => IoResult::Failed(IoFailure::Process(failure)),
                };
                (id, result)
            }
        };
        if completions.send(IoCompletion { id, result }).is_err() {
            break;
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
    http_agent: &ureq::Agent,
) -> IoResult {
    match request {
        IoRequest::Print(text) => write_stdout(stdout, &text),
        IoRequest::ReadLine => perform_read_line(stdin),
        IoRequest::ReadFile { path, maximum } => perform_read_file(path, maximum, true),
        IoRequest::ReadFileBytes { path, maximum } => perform_read_file(path, maximum, false),
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
        IoRequest::Process(_) | IoRequest::StartProcess(_) => {
            solveig_base::ice!("process request reached the non-cancellable worker path")
        }
        IoRequest::ResolveHost(host) => perform_resolve_host(host),
        IoRequest::Http(request) => match super::http::perform(http_agent, request) {
            Ok(response) => IoResult::Http(response),
            Err(failure) => IoResult::Failed(IoFailure::Http(failure)),
        },
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
        | IoRequest::CloseSocket(_)
        | IoRequest::ReadProcessStdout { .. }
        | IoRequest::ReadProcessStderr { .. }
        | IoRequest::WriteProcessStdin { .. }
        | IoRequest::CloseProcessStdin(_)
        | IoRequest::WaitProcess(_)
        | IoRequest::TerminateProcess(_)
        | IoRequest::CloseProcess(_) => {
            solveig_base::ice!("direct IO request reached the blocking worker pool")
        }
    }
}

fn perform_read_file(path: PathBuf, maximum: Option<i64>, text: bool) -> IoResult {
    let contents = match maximum {
        None => fs::read(&path).map_err(|source| IoFailure::ReadFile {
            path: path.clone(),
            source,
        }),
        Some(maximum) => read_file_limited(&path, maximum),
    };
    let contents = match contents {
        Ok(contents) => contents,
        Err(failure) => return IoResult::Failed(failure),
    };
    if !text {
        return IoResult::Bytes(contents);
    }
    match String::from_utf8(contents) {
        Ok(contents) => IoResult::Text(contents),
        Err(error) => {
            let source = io::Error::new(io::ErrorKind::InvalidData, error);
            let failure = if maximum.is_some() {
                IoFailure::ReadFileLimited { path, source }
            } else {
                IoFailure::ReadFile { path, source }
            };
            IoResult::Failed(failure)
        }
    }
}

fn read_file_limited(path: &Path, maximum: i64) -> Result<Vec<u8>, IoFailure> {
    if maximum < 0 {
        return Err(IoFailure::ReadFileLimited {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "maximum byte count must be non-negative",
            ),
        });
    }
    let maximum = usize::try_from(maximum).map_err(|_out_of_range| IoFailure::ReadFileLimited {
        path: path.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::InvalidInput,
            "maximum byte count is not representable by this host",
        ),
    })?;
    let read_limit = maximum
        .checked_add(1)
        .ok_or_else(|| IoFailure::ReadFileLimited {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "maximum byte count is not representable by this host",
            ),
        })?;
    let file = fs::File::open(path).map_err(|source| IoFailure::ReadFileLimited {
        path: path.to_path_buf(),
        source,
    })?;
    let mut contents = Vec::new();
    file.take(read_limit as u64)
        .read_to_end(&mut contents)
        .map_err(|source| IoFailure::ReadFileLimited {
            path: path.to_path_buf(),
            source,
        })?;
    if contents.len() > maximum {
        return Err(IoFailure::ReadFileTooLarge {
            path: path.to_path_buf(),
            maximum: maximum as i64,
        });
    }
    Ok(contents)
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
