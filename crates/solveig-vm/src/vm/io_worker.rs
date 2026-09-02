//! Scheduler-facing IO routing, cancellation, and request/completion ownership.
//! Blocking workers, sockets, and live children exchange plain Rust data here;
//! no VM values or heap references cross the host boundary.

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

mod blocking;
mod request;
use blocking::worker_loop;
use request::IoWork;
pub(super) use request::{IoFailure, IoRequest, IoResult};

pub(super) mod filesystem;
pub(super) mod process;
mod streaming_http_upload;
pub(super) mod streaming_process;

use filesystem::{FilesystemFailure, FilesystemRequest};
use process::{ProcessFailure, ProcessOutput, ProcessRequest, ProcessSignal};
use streaming_http_upload::StreamingHttpUpload;
use streaming_process::StreamingProcess;

use super::socket::{SocketRequest, SocketSubsystem};

// Worker zero preserves terminal ordering; four general workers provide the
// bounded host concurrency used by std.http and other blocking operations.
const IO_WORKER_COUNT: usize = 5;

enum IoCancellation {
    StrictProcess(mpsc::Sender<ProcessSignal>),
    StreamingProcess(ChildProcessId),
    StreamingHttpUpload(HandleId),
}

struct RequestSlot {
    owner: Option<RequestOwner>,
    cancellation: Option<IoCancellation>,
    closing_process: Option<StreamingProcess>,
    closing_http_upload: Option<StreamingHttpUpload>,
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
    completion_tx: mpsc::Sender<IoCompletion>,
    requests: Vec<RequestSlot>,
    streaming_processes: Vec<Option<StreamingProcess>>,
    streaming_http_uploads: Vec<Option<StreamingHttpUpload>>,
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
        let (completion_tx, completions) = mpsc::channel();
        Self {
            workers: Vec::new(),
            sockets: SocketSubsystem::inactive(),
            completions,
            completion_tx,
            requests: Vec::new(),
            streaming_processes: Vec::new(),
            streaming_http_uploads: (0..FIRST_FILE_HANDLE).map(|_| None).collect(),
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
        Ok(Self {
            workers,
            sockets,
            completions,
            completion_tx,
            requests: Vec::new(),
            streaming_processes: Vec::new(),
            streaming_http_uploads: (0..FIRST_FILE_HANDLE).map(|_| None).collect(),
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
            | IoRequest::CloseSocket(_)
            | IoRequest::StartHttpUpload { .. }
            | IoRequest::ContinueHttpUpload { .. }
            | IoRequest::CloseHttpUpload(_) => 0,
            IoRequest::Handle(request) => {
                let handle = request.handle();
                if self.streaming_http_upload(handle).is_ok() {
                    0
                } else {
                    self.handle_workers
                        .get(handle.0 as usize)
                        .and_then(|worker| *worker)
                        .ok_or_else(|| {
                            RuntimeError::builtin(format!("unknown Handle identity {}", handle.0))
                        })?
                }
            }
            IoRequest::OpenHttp { handle, .. } => self
                .handle_workers
                .get(handle.0 as usize)
                .and_then(|worker| *worker)
                .expect("ICE: reserved HTTP handle has a worker"),
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
            | IoRequest::SecureRandomBytes(_)
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
            if slot.closing_http_upload.is_some() {
                solveig_base::ice!("free IO request retained a closing HTTP upload");
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
                closing_http_upload: None,
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
            IoRequest::StartHttpUpload { handle, request } => {
                let resource =
                    streaming_http_upload::start(handle, request, id, self.completion_tx.clone())
                        .map_err(|error| {
                        RuntimeError::builtin(format!("failed to start HTTP upload: {error}"))
                    })?;
                let Some(slot) = self.streaming_http_uploads.get_mut(handle.0 as usize) else {
                    solveig_base::ice!("reserved HTTP upload handle vanished")
                };
                *slot = Some(resource);
                self.track_http_upload_cancellation(id, handle);
                Ok(())
            }
            IoRequest::ContinueHttpUpload { handle, bytes } => {
                self.streaming_http_upload(handle)?
                    .supply(id, bytes)
                    .map_err(|error| streaming_http_upload_issue_error(&error))?;
                self.track_http_upload_cancellation(id, handle);
                Ok(())
            }
            IoRequest::CloseHttpUpload(handle) => self.issue_close_http_upload_resource(id, handle),
            IoRequest::Handle(HandleRequest::GetBytes { handle, maximum })
                if self.streaming_http_upload(handle).is_ok() =>
            {
                self.streaming_http_upload(handle)?
                    .read(id, maximum)
                    .map_err(|error| streaming_http_upload_issue_error(&error))?;
                self.track_http_upload_cancellation(id, handle);
                Ok(())
            }
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
            if let Some(upload) = slot.closing_http_upload.take() {
                upload.shutdown_and_join();
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

    fn track_http_upload_cancellation(&mut self, request: RequestId, handle: HandleId) {
        let Some(slot) = self.requests.get_mut(request.0 as usize) else {
            solveig_base::ice!("HTTP upload request had no request-table slot");
        };
        slot.cancellation = Some(IoCancellation::StreamingHttpUpload(handle));
    }

    fn streaming_http_upload(
        &self,
        handle: HandleId,
    ) -> Result<&StreamingHttpUpload, RuntimeError> {
        self.streaming_http_uploads
            .get(handle.0 as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| {
                RuntimeError::builtin(format!("unknown HTTP upload Handle identity {}", handle.0))
            })
    }

    fn issue_close_http_upload_resource(
        &mut self,
        request: RequestId,
        handle: HandleId,
    ) -> Result<(), RuntimeError> {
        let Some(slot) = self.streaming_http_uploads.get_mut(handle.0 as usize) else {
            return Err(unknown_http_upload(handle));
        };
        let Some(resource) = slot.take() else {
            return Err(unknown_http_upload(handle));
        };
        resource
            .close(request)
            .map_err(|error| streaming_http_upload_issue_error(&error))?;
        let Some(request_slot) = self.requests.get_mut(request.0 as usize) else {
            solveig_base::ice!("close-upload request had no request-table slot");
        };
        request_slot.closing_http_upload = Some(resource);
        Ok(())
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
        let worker = self.next_worker % self.workers.len();
        let handle = self.reserve_handle(worker)?;
        self.issue(
            process,
            IoRequest::Handle(HandleRequest::OpenFile { handle, path, mode }),
        )
    }

    pub(super) fn issue_open_http(
        &mut self,
        process: ProcessId,
        request: HttpRequest,
    ) -> Result<RequestId, RuntimeError> {
        let worker = 1 + self.next_worker % self.workers.len().saturating_sub(1);
        let handle = self.reserve_handle(worker)?;
        self.issue(process, IoRequest::OpenHttp { handle, request })
    }

    pub(super) fn issue_start_http_upload(
        &mut self,
        process: ProcessId,
        request: HttpRequest,
    ) -> Result<RequestId, RuntimeError> {
        let handle = self.reserve_http_upload_handle()?;
        self.issue(process, IoRequest::StartHttpUpload { handle, request })
    }

    pub(super) fn issue_continue_http_upload(
        &mut self,
        process: ProcessId,
        handle: HandleId,
        bytes: Option<Vec<u8>>,
    ) -> Result<RequestId, RuntimeError> {
        self.issue(process, IoRequest::ContinueHttpUpload { handle, bytes })
    }

    pub(super) fn issue_close_http_upload(
        &mut self,
        process: ProcessId,
        handle: HandleId,
    ) -> Result<RequestId, RuntimeError> {
        self.issue(process, IoRequest::CloseHttpUpload(handle))
    }

    fn reserve_handle(&mut self, worker: usize) -> Result<HandleId, RuntimeError> {
        let raw = u32::try_from(self.handle_workers.len()).map_err(|_too_many_handles| {
            RuntimeError::builtin("handle table exhausted".to_string())
        })?;
        self.handle_workers.push(Some(worker));
        self.streaming_http_uploads.push(None);
        Ok(HandleId(raw))
    }

    fn reserve_http_upload_handle(&mut self) -> Result<HandleId, RuntimeError> {
        let raw = u32::try_from(self.handle_workers.len()).map_err(|_too_many_handles| {
            RuntimeError::builtin("handle table exhausted".to_string())
        })?;
        self.handle_workers.push(None);
        self.streaming_http_uploads.push(None);
        Ok(HandleId(raw))
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
                if let Some(upload) = slot.closing_http_upload.as_ref() {
                    upload.cancel(id);
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
                IoCancellation::StreamingHttpUpload(handle) => {
                    if let Ok(upload) = self.streaming_http_upload(handle) {
                        upload.cancel(id);
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
        if let Some(upload) = slot.closing_http_upload.take() {
            upload.join()?;
        }
        self.free_requests.push(completion.id.0);
        Ok(match (owner, completion.result) {
            (RequestOwner::Process(process), result) => Some((completion.id, process, result)),
            (RequestOwner::Cancelled, IoResult::StreamingProcess(process)) => {
                process.shutdown_and_join();
                None
            }
            (RequestOwner::Cancelled, IoResult::HttpOpened(response)) => {
                let worker = self
                    .handle_workers
                    .get(response.body.0 as usize)
                    .and_then(|worker| *worker)
                    .and_then(|index| self.workers.get(index))
                    .expect("ICE: acquired HTTP handle has a live worker");
                worker
                    .send(IoWork::CloseHttp(response.body))
                    .map_err(|_disconnected| {
                        RuntimeError::builtin(
                            "IO worker stopped before cancelled HTTP cleanup".to_string(),
                        )
                    })?;
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
            if let Some(upload) = slot.closing_http_upload.take() {
                upload.shutdown_and_join();
            }
        }
        for process in self.streaming_processes.drain(..).flatten() {
            process.shutdown_and_join();
        }
        for upload in self.streaming_http_uploads.drain(..).flatten() {
            upload.shutdown_and_join();
        }
    }
}

fn unknown_streaming_process(process: ChildProcessId) -> RuntimeError {
    RuntimeError::builtin(format!("unknown ChildProcess identity {}", process.0))
}

fn streaming_issue_error(error: &io::Error) -> RuntimeError {
    RuntimeError::builtin(format!("failed to issue child-process IO: {error}"))
}

fn unknown_http_upload(handle: HandleId) -> RuntimeError {
    RuntimeError::builtin(format!("unknown HTTP upload Handle identity {}", handle.0))
}

fn streaming_http_upload_issue_error(error: &io::Error) -> RuntimeError {
    RuntimeError::builtin(format!("failed to issue HTTP upload IO: {error}"))
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
