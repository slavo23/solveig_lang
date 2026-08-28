//! Scoped live child processes with demand-driven pipe IO.
//!
//! One manager thread retains sole `Child` ownership. Independent scoped pipe
//! threads receive one command per Solveig pull/write, so stdout and stderr can
//! make progress concurrently without reading ahead or introducing an async
//! runtime. Cleanup joins every pipe thread before reporting completion.

use super::process::{ProcessInput, ProcessRequest, terminate_and_reap_status};
use super::*;
use std::process::{Child, ChildStdin, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;

const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(1);

enum ReadCommand {
    Read { id: RequestId, maximum: usize },
    Shutdown,
}

enum InputCommand {
    Write { id: RequestId, bytes: Vec<u8> },
    Close { id: RequestId },
    Shutdown,
}

#[derive(Clone)]
struct PipeFault {
    operation: &'static str,
    kind: io::ErrorKind,
    message: String,
}

impl PipeFault {
    fn from_error(operation: &'static str, source: &io::Error) -> Self {
        Self {
            operation,
            kind: source.kind(),
            message: source.to_string(),
        }
    }

    fn failure(&self) -> IoFailure {
        IoFailure::ProcessStream {
            operation: self.operation,
            source: io::Error::new(self.kind, self.message.clone()),
        }
    }
}

enum ControlCommand {
    Wait(RequestId),
    Terminate(RequestId),
    Close(RequestId),
    Cancel,
    Shutdown,
    PipeFailed(PipeFault),
}

struct ProcessEndpoints {
    stdin: Option<Sender<InputCommand>>,
    stdout: Option<Sender<ReadCommand>>,
    stderr: Option<Sender<ReadCommand>>,
}

pub(in crate::vm) struct StreamingProcess {
    completions: Sender<IoCompletion>,
    control: Sender<ControlCommand>,
    stdin: Option<Sender<InputCommand>>,
    stdout: Option<Sender<ReadCommand>>,
    stderr: Option<Sender<ReadCommand>>,
    manager: Option<JoinHandle<()>>,
}

impl StreamingProcess {
    pub(super) fn read_stdout(&self, id: RequestId, maximum: i64) -> io::Result<()> {
        send_read(&self.stdout, id, maximum, "readProcessStdout")
    }

    pub(super) fn read_stderr(&self, id: RequestId, maximum: i64) -> io::Result<()> {
        send_read(&self.stderr, id, maximum, "readProcessStderr")
    }

    pub(super) fn write_stdin(&self, id: RequestId, bytes: Vec<u8>) -> io::Result<()> {
        let Some(stdin) = &self.stdin else {
            return self.complete_failure(
                id,
                "writeProcessStdin",
                io::Error::new(io::ErrorKind::BrokenPipe, "child standard input is closed"),
            );
        };
        stdin
            .send(InputCommand::Write { id, bytes })
            .map_err(|_disconnected| closed_manager("writeProcessStdin"))
    }

    pub(super) fn close_stdin(&mut self, id: RequestId) -> io::Result<()> {
        let Some(stdin) = self.stdin.take() else {
            return self
                .completions
                .send(IoCompletion {
                    id,
                    result: IoResult::Unit,
                })
                .map_err(|_disconnected| closed_manager("closeProcessStdin"));
        };
        stdin
            .send(InputCommand::Close { id })
            .map_err(|_disconnected| closed_manager("closeProcessStdin"))
    }

    pub(super) fn wait(&self, id: RequestId) -> io::Result<()> {
        self.control
            .send(ControlCommand::Wait(id))
            .map_err(|_disconnected| closed_manager("waitProcess"))
    }

    pub(super) fn terminate(&self, id: RequestId) -> io::Result<()> {
        self.control
            .send(ControlCommand::Terminate(id))
            .map_err(|_disconnected| closed_manager("terminateProcess"))
    }

    pub(super) fn close(&self, id: RequestId) -> io::Result<()> {
        self.control
            .send(ControlCommand::Close(id))
            .map_err(|_disconnected| closed_manager("closeProcess"))
    }

    pub(super) fn cancel(&self) {
        drop(self.control.send(ControlCommand::Cancel));
    }

    pub(super) fn shutdown_and_join(mut self) {
        drop(self.control.send(ControlCommand::Shutdown));
        if let Some(manager) = self.manager.take() {
            drop(manager.join());
        }
    }

    pub(super) fn join(mut self) -> Result<(), RuntimeError> {
        let Some(manager) = self.manager.take() else {
            solveig_base::ice!("streaming process manager was joined more than once")
        };
        manager.join().map_err(|_panic| {
            RuntimeError::builtin("streaming process manager panicked during cleanup".to_string())
        })
    }

    fn complete_failure(
        &self,
        id: RequestId,
        operation: &'static str,
        source: io::Error,
    ) -> io::Result<()> {
        self.completions
            .send(IoCompletion {
                id,
                result: IoResult::Failed(IoFailure::ProcessStream { operation, source }),
            })
            .map_err(|_disconnected| closed_manager(operation))
    }
}

pub(super) fn start(
    request: ProcessRequest,
    completions: Sender<IoCompletion>,
    signals: &Receiver<ProcessSignal>,
) -> Result<StreamingProcess, ProcessFailure> {
    let program = request.program.clone();
    start_inner(request, completions, signals).map_err(|source| ProcessFailure { program, source })
}

fn start_inner(
    request: ProcessRequest,
    completions: Sender<IoCompletion>,
    signals: &Receiver<ProcessSignal>,
) -> io::Result<StreamingProcess> {
    match signals.try_recv() {
        Ok(ProcessSignal::Cancel) | Err(mpsc::TryRecvError::Disconnected) => {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "child process was cancelled before it started",
            ));
        }
        Ok(ProcessSignal::PipeFailed) => {
            solveig_base::ice!("streaming process received a pipe failure before spawn")
        }
        Err(mpsc::TryRecvError::Empty) => {}
    }

    let ProcessRequest {
        executable,
        arguments,
        cwd,
        environment,
        input,
        capture_stdout,
        capture_stderr,
        ..
    } = request;
    let (strict_input, streamed_input) = match input {
        ProcessInput::Strict(bytes) => (bytes, false),
        ProcessInput::Streamed => (Vec::new(), true),
    };
    let child_stdin = if streamed_input || !strict_input.is_empty() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    let mut child = std::process::Command::new(executable)
        .args(arguments)
        .current_dir(cwd)
        .env_clear()
        .envs(environment)
        .stdin(child_stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let child_input = child.stdin.take();
    let Some(stdout) = child.stdout.take() else {
        terminate_and_reap_status(&mut child)?;
        return Err(io::Error::other(
            "spawned child did not provide its standard-output pipe",
        ));
    };
    let Some(stderr) = child.stderr.take() else {
        terminate_and_reap_status(&mut child)?;
        return Err(io::Error::other(
            "spawned child did not provide its standard-error pipe",
        ));
    };

    let (control, controls) = mpsc::channel();
    let (setup_tx, setup_rx) = mpsc::sync_channel(1);
    let manager_completions = completions.clone();
    let manager_control = control.clone();
    let manager = thread::Builder::new()
        .name("solveig-child-manager".to_string())
        .spawn(move || {
            let close_completion = thread::scope(|scope| {
                let stdout_setup = spawn_output_pipe(
                    scope,
                    stdout,
                    capture_stdout,
                    "readProcessStdout",
                    manager_completions.clone(),
                    manager_control.clone(),
                );
                let stdout_commands = match stdout_setup {
                    Ok(commands) => commands,
                    Err(error) => {
                        terminate_after_setup_failure(&mut child);
                        drop(setup_tx.send(Err(error)));
                        return None;
                    }
                };
                let stdout_shutdown = stdout_commands.clone();
                let stderr_setup = spawn_output_pipe(
                    scope,
                    stderr,
                    capture_stderr,
                    "readProcessStderr",
                    manager_completions.clone(),
                    manager_control.clone(),
                );
                let stderr_commands = match stderr_setup {
                    Ok(commands) => commands,
                    Err(error) => {
                        shutdown_reader(&stdout_shutdown);
                        terminate_after_setup_failure(&mut child);
                        drop(setup_tx.send(Err(error)));
                        return None;
                    }
                };
                let stderr_shutdown = stderr_commands.clone();
                let input_setup = spawn_input_pipe(
                    scope,
                    child_input,
                    strict_input,
                    streamed_input,
                    manager_completions.clone(),
                    manager_control.clone(),
                );
                let stdin_commands = match input_setup {
                    Ok(commands) => commands,
                    Err(error) => {
                        shutdown_reader(&stdout_shutdown);
                        shutdown_reader(&stderr_shutdown);
                        terminate_after_setup_failure(&mut child);
                        drop(setup_tx.send(Err(error)));
                        return None;
                    }
                };
                let stdin_shutdown = stdin_commands.clone();
                let endpoints = ProcessEndpoints {
                    stdin: stdin_commands,
                    stdout: stdout_commands,
                    stderr: stderr_commands,
                };
                if setup_tx.send(Ok(endpoints)).is_err() {
                    shutdown_input(&stdin_shutdown);
                    shutdown_reader(&stdout_shutdown);
                    shutdown_reader(&stderr_shutdown);
                    terminate_after_setup_failure(&mut child);
                    return None;
                }

                let close_completion = manage_child(&mut child, &controls, &manager_completions);
                shutdown_input(&stdin_shutdown);
                shutdown_reader(&stdout_shutdown);
                shutdown_reader(&stderr_shutdown);
                close_completion
            });
            if let Some((id, result)) = close_completion {
                drop(manager_completions.send(IoCompletion { id, result }));
            }
        })?;

    let endpoints = match setup_rx.recv() {
        Ok(Ok(endpoints)) => endpoints,
        Ok(Err(error)) => {
            drop(manager.join());
            return Err(error);
        }
        Err(_disconnected) => {
            drop(manager.join());
            return Err(io::Error::other(
                "streaming process manager stopped during pipe setup",
            ));
        }
    };

    match signals.try_recv() {
        Ok(ProcessSignal::Cancel) | Err(mpsc::TryRecvError::Disconnected) => {
            drop(control.send(ControlCommand::Shutdown));
            drop(manager.join());
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "child process was cancelled while it started",
            ));
        }
        Ok(ProcessSignal::PipeFailed) | Err(mpsc::TryRecvError::Empty) => {}
    }

    Ok(StreamingProcess {
        completions,
        control,
        stdin: endpoints.stdin,
        stdout: endpoints.stdout,
        stderr: endpoints.stderr,
        manager: Some(manager),
    })
}

fn spawn_output_pipe<'scope>(
    scope: &'scope thread::Scope<'scope, '_>,
    stream: impl Read + Send + 'scope,
    captured: bool,
    operation: &'static str,
    completions: Sender<IoCompletion>,
    control: Sender<ControlCommand>,
) -> io::Result<Option<Sender<ReadCommand>>> {
    if !captured {
        let mut stream = stream;
        thread::Builder::new()
            .name(format!("solveig-{operation}-discard"))
            .spawn_scoped(scope, move || drain_pipe(&mut stream, operation, &control))?;
        return Ok(None);
    }
    let (commands, requests) = mpsc::channel();
    thread::Builder::new()
        .name(format!("solveig-{operation}"))
        .spawn_scoped(scope, move || {
            read_pipe(stream, operation, &requests, &completions, &control);
        })?;
    Ok(Some(commands))
}

fn spawn_input_pipe<'scope>(
    scope: &'scope thread::Scope<'scope, '_>,
    stream: Option<ChildStdin>,
    strict_input: Vec<u8>,
    streamed: bool,
    completions: Sender<IoCompletion>,
    control: Sender<ControlCommand>,
) -> io::Result<Option<Sender<InputCommand>>> {
    let Some(stream) = stream else {
        return Ok(None);
    };
    if !streamed {
        let mut stream = stream;
        thread::Builder::new()
            .name("solveig-child-stdin-strict".to_string())
            .spawn_scoped(scope, move || {
                write_strict_input(&mut stream, &strict_input, &control);
            })?;
        return Ok(None);
    }
    let (commands, requests) = mpsc::channel();
    thread::Builder::new()
        .name("solveig-child-stdin".to_string())
        .spawn_scoped(scope, move || {
            write_streamed_input(stream, &requests, &completions, &control);
        })?;
    Ok(Some(commands))
}

fn manage_child(
    child: &mut Child,
    controls: &Receiver<ControlCommand>,
    completions: &Sender<IoCompletion>,
) -> Option<(RequestId, IoResult)> {
    let mut status = None;
    let mut fault = None;
    let mut waiters = Vec::new();
    loop {
        if status.is_none() {
            match child.try_wait() {
                Ok(Some(completed)) => status = Some(completed),
                Ok(None) => {}
                Err(source) => {
                    fault.get_or_insert_with(|| PipeFault::from_error("waitProcess", &source));
                }
            }
        }
        complete_waiters(&mut waiters, status.as_ref(), fault.as_ref(), completions);

        let command = if status.is_some() || fault.is_some() {
            controls
                .recv()
                .map_err(|_disconnected| RecvTimeoutError::Disconnected)
        } else {
            controls.recv_timeout(CHILD_POLL_INTERVAL)
        };
        match command {
            Ok(ControlCommand::Wait(id)) => waiters.push(id),
            Ok(ControlCommand::Terminate(id)) => {
                let result = ensure_terminated(child, &mut status);
                complete_control(id, "terminateProcess", result, completions);
            }
            Ok(ControlCommand::Cancel) => {
                drop(ensure_terminated(child, &mut status));
            }
            Ok(ControlCommand::PipeFailed(pipe_fault)) => {
                fault.get_or_insert(pipe_fault);
                drop(ensure_terminated(child, &mut status));
            }
            Ok(ControlCommand::Close(id)) => {
                let result = ensure_terminated(child, &mut status).map_or_else(
                    |source| {
                        IoResult::Failed(IoFailure::ProcessStream {
                            operation: "closeProcess",
                            source,
                        })
                    },
                    |_status| IoResult::Unit,
                );
                return Some((id, result));
            }
            Ok(ControlCommand::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                drop(ensure_terminated(child, &mut status));
                return None;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn complete_waiters(
    waiters: &mut Vec<RequestId>,
    status: Option<&ExitStatus>,
    fault: Option<&PipeFault>,
    completions: &Sender<IoCompletion>,
) {
    let result = if let Some(fault) = fault {
        Some(Err(fault))
    } else {
        status.map(Ok)
    };
    let Some(result) = result else {
        return;
    };
    for id in waiters.drain(..) {
        let result = match result {
            Ok(status) => IoResult::Int(exit_code(status)),
            Err(fault) => IoResult::Failed(fault.failure()),
        };
        drop(completions.send(IoCompletion { id, result }));
    }
}

fn complete_control(
    id: RequestId,
    operation: &'static str,
    result: io::Result<ExitStatus>,
    completions: &Sender<IoCompletion>,
) {
    let result = match result {
        Ok(status) => IoResult::Int(exit_code(&status)),
        Err(source) => IoResult::Failed(IoFailure::ProcessStream { operation, source }),
    };
    drop(completions.send(IoCompletion { id, result }));
}

fn ensure_terminated(child: &mut Child, status: &mut Option<ExitStatus>) -> io::Result<ExitStatus> {
    if let Some(status) = status {
        return Ok(*status);
    }
    let completed = terminate_and_reap_status(child)?;
    *status = Some(completed);
    Ok(completed)
}

fn read_pipe(
    mut stream: impl Read,
    operation: &'static str,
    commands: &Receiver<ReadCommand>,
    completions: &Sender<IoCompletion>,
    control: &Sender<ControlCommand>,
) {
    while let Ok(command) = commands.recv() {
        let ReadCommand::Read { id, maximum } = command else {
            break;
        };
        let result = read_chunk(&mut stream, maximum).map_or_else(
            |source| {
                let fault = PipeFault::from_error(operation, &source);
                drop(control.send(ControlCommand::PipeFailed(fault)));
                IoResult::Failed(IoFailure::ProcessStream { operation, source })
            },
            IoResult::Bytes,
        );
        if completions.send(IoCompletion { id, result }).is_err() {
            break;
        }
    }
}

fn read_chunk(stream: &mut impl Read, maximum: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(maximum).map_err(|error| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            format!("cannot reserve a {maximum}-byte process pipe chunk: {error}"),
        )
    })?;
    bytes.resize(maximum, 0);
    let count = stream.read(&mut bytes)?;
    bytes.truncate(count);
    Ok(bytes)
}

fn drain_pipe(stream: &mut impl Read, operation: &'static str, control: &Sender<ControlCommand>) {
    if let Err(source) = io::copy(stream, &mut io::sink()) {
        drop(
            control.send(ControlCommand::PipeFailed(PipeFault::from_error(
                operation, &source,
            ))),
        );
    }
}

fn write_strict_input(stream: &mut ChildStdin, input: &[u8], control: &Sender<ControlCommand>) {
    if let Err(source) = stream.write_all(input).and_then(|()| stream.flush()) {
        drop(
            control.send(ControlCommand::PipeFailed(PipeFault::from_error(
                "writeProcessStdin",
                &source,
            ))),
        );
    }
}

fn write_streamed_input(
    mut stream: ChildStdin,
    commands: &Receiver<InputCommand>,
    completions: &Sender<IoCompletion>,
    control: &Sender<ControlCommand>,
) {
    while let Ok(command) = commands.recv() {
        let (id, operation, result) = match command {
            InputCommand::Write { id, bytes } => (
                id,
                "writeProcessStdin",
                stream.write_all(&bytes).and_then(|()| stream.flush()),
            ),
            InputCommand::Close { id } => {
                let result = stream.flush();
                drop(stream);
                complete_pipe_operation(id, "closeProcessStdin", result, completions, control);
                return;
            }
            InputCommand::Shutdown => break,
        };
        if !complete_pipe_operation(id, operation, result, completions, control) {
            break;
        }
    }
}

fn complete_pipe_operation(
    id: RequestId,
    operation: &'static str,
    result: io::Result<()>,
    completions: &Sender<IoCompletion>,
    control: &Sender<ControlCommand>,
) -> bool {
    let result = result.map_or_else(
        |source| {
            let fault = PipeFault::from_error(operation, &source);
            drop(control.send(ControlCommand::PipeFailed(fault)));
            IoResult::Failed(IoFailure::ProcessStream { operation, source })
        },
        |()| IoResult::Unit,
    );
    completions.send(IoCompletion { id, result }).is_ok()
}

fn send_read(
    pipe: &Option<Sender<ReadCommand>>,
    id: RequestId,
    maximum: i64,
    operation: &'static str,
) -> io::Result<()> {
    let maximum = usize::try_from(maximum)
        .ok()
        .filter(|maximum| *maximum > 0)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "process pipe chunk size must be a positive host-representable Int",
            )
        })?;
    let Some(pipe) = pipe else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{operation} requested a discarded process stream"),
        ));
    };
    pipe.send(ReadCommand::Read { id, maximum })
        .map_err(|_disconnected| closed_manager(operation))
}

fn shutdown_reader(pipe: &Option<Sender<ReadCommand>>) {
    if let Some(pipe) = pipe {
        drop(pipe.send(ReadCommand::Shutdown));
    }
}

fn shutdown_input(pipe: &Option<Sender<InputCommand>>) {
    if let Some(pipe) = pipe {
        drop(pipe.send(InputCommand::Shutdown));
    }
}

fn terminate_after_setup_failure(child: &mut Child) {
    drop(terminate_and_reap_status(child));
}

fn exit_code(status: &ExitStatus) -> i64 {
    status.code().map_or(-1, i64::from)
}

fn closed_manager(operation: &'static str) -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        format!("{operation} lost its child-process manager"),
    )
}
