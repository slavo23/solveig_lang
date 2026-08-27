//! Cancellable strict child-process capture on the bounded blocking worker pool.

use super::*;
use std::process::{Child, Command, ExitStatus as ChildExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(1);
const CAPTURE_CHUNK_BYTES: usize = 8 * 1024;

pub(crate) struct ProcessRequest {
    pub program: String,
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub cwd: PathBuf,
    pub environment: Vec<(String, String)>,
    pub stdin: Vec<u8>,
    pub capture_stdout: bool,
    pub capture_stderr: bool,
    pub maximum: Option<i64>,
}

pub(crate) struct ProcessOutput {
    pub exit_code: i64,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub(crate) struct ProcessFailure {
    pub program: String,
    pub source: io::Error,
}

pub(super) enum ProcessSignal {
    Cancel,
    PipeFailed,
}

pub(super) fn perform(
    request: ProcessRequest,
    signals: &Receiver<ProcessSignal>,
    signal_tx: Sender<ProcessSignal>,
) -> IoResult {
    let program = request.program.clone();
    match capture(request, signals, signal_tx) {
        Ok(output) => IoResult::Process(output),
        Err(source) => IoResult::Failed(IoFailure::Process(ProcessFailure { program, source })),
    }
}

fn capture(
    request: ProcessRequest,
    signals: &Receiver<ProcessSignal>,
    signal_tx: Sender<ProcessSignal>,
) -> io::Result<ProcessOutput> {
    let ProcessRequest {
        executable,
        arguments,
        cwd,
        environment,
        stdin,
        capture_stdout,
        capture_stderr,
        maximum,
        ..
    } = request;
    let maximum = capture_limit(maximum)?;

    match signals.try_recv() {
        Ok(ProcessSignal::Cancel) | Err(mpsc::TryRecvError::Disconnected) => {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "child process was cancelled before it started",
            ));
        }
        Ok(ProcessSignal::PipeFailed) => {
            solveig_base::ice!("process pipe failed before its worker threads started")
        }
        Err(mpsc::TryRecvError::Empty) => {}
    }

    let child_stdin = if stdin.is_empty() {
        Stdio::null()
    } else {
        Stdio::piped()
    };
    let mut child = Command::new(executable)
        .args(arguments)
        .current_dir(cwd)
        .env_clear()
        .envs(environment)
        .stdin(child_stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let Some(stdout) = child.stdout.take() else {
        terminate_and_reap(&mut child)?;
        return Err(io::Error::other(
            "spawned child did not provide its configured standard-output pipe",
        ));
    };
    let Some(stderr) = child.stderr.take() else {
        terminate_and_reap(&mut child)?;
        return Err(io::Error::other(
            "spawned child did not provide its configured standard-error pipe",
        ));
    };

    // Rust's `Child::wait_with_output` drains both pipes concurrently to avoid
    // pipe-capacity deadlock. Keep that invariant while the owning worker polls
    // the scheduler's cancellation signal and retains the `Child` to reap it.
    thread::scope(|scope| {
        let stdout_signals = signal_tx.clone();
        let stdin_signals = signal_tx.clone();
        let stdout_reader = scope.spawn(move || {
            read_capture(
                stdout,
                maximum,
                capture_stdout,
                "standard output",
                &stdout_signals,
            )
        });
        let stderr_reader = scope.spawn(move || {
            read_capture(
                stderr,
                maximum,
                capture_stderr,
                "standard error",
                &signal_tx,
            )
        });
        let stdin_writer = child.stdin.take().map(|mut child_stdin| {
            scope.spawn(move || write_input(&mut child_stdin, &stdin, &stdin_signals))
        });

        let status = wait_for_child(&mut child, signals);
        if let Some(writer) = stdin_writer {
            writer
                .join()
                .map_err(|_panic| io::Error::other("child standard-input writer panicked"))??;
        }
        let stdout = stdout_reader
            .join()
            .map_err(|_panic| io::Error::other("child standard-output reader panicked"))?;
        let stderr = stderr_reader
            .join()
            .map_err(|_panic| io::Error::other("child standard-error reader panicked"))?;

        let stdout = stdout?;
        let stderr = stderr?;
        let Some(status) = status? else {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "child process was cancelled",
            ));
        };
        Ok(ProcessOutput {
            exit_code: status.code().map_or(-1, i64::from),
            stdout,
            stderr,
        })
    })
}

fn write_input(
    stream: &mut impl Write,
    input: &[u8],
    signals: &Sender<ProcessSignal>,
) -> io::Result<()> {
    let result = stream.write_all(input).and_then(|()| stream.flush());
    if result.is_err() {
        drop(signals.send(ProcessSignal::PipeFailed));
    }
    result
}

fn capture_limit(maximum: Option<i64>) -> io::Result<Option<usize>> {
    let Some(maximum) = maximum else {
        return Ok(None);
    };
    if maximum < 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "maximum child-output byte count must be non-negative",
        ));
    }
    usize::try_from(maximum).map(Some).map_err(|_out_of_range| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "maximum child-output byte count is not representable by this host",
        )
    })
}

fn read_capture(
    mut stream: impl Read,
    maximum: Option<usize>,
    retain: bool,
    stream_name: &'static str,
    signals: &Sender<ProcessSignal>,
) -> io::Result<Vec<u8>> {
    let result = if retain {
        read_capture_inner(&mut stream, maximum, stream_name)
    } else {
        io::copy(&mut stream, &mut io::sink()).map(|_copied| Vec::new())
    };
    if result.is_err() {
        drop(signals.send(ProcessSignal::PipeFailed));
    }
    result
}

fn read_capture_inner(
    stream: &mut impl Read,
    maximum: Option<usize>,
    stream_name: &'static str,
) -> io::Result<Vec<u8>> {
    let Some(maximum) = maximum else {
        let mut output = Vec::new();
        stream.read_to_end(&mut output)?;
        return Ok(output);
    };

    let mut output = Vec::new();
    let mut chunk = [0; CAPTURE_CHUNK_BYTES];
    loop {
        let remaining = maximum.saturating_sub(output.len());
        let probe = remaining.saturating_add(1).min(chunk.len());
        let Some(buffer) = chunk.get_mut(..probe) else {
            solveig_base::ice!("child capture probe exceeded its fixed read buffer");
        };
        let read = stream.read(buffer)?;
        if read == 0 {
            return Ok(output);
        }
        if read > remaining {
            return Err(io::Error::other(format!(
                "child {stream_name} exceeds the configured limit of {maximum} bytes"
            )));
        }
        let Some(bytes) = chunk.get(..read) else {
            solveig_base::ice!("child capture read exceeded the supplied buffer");
        };
        output.extend_from_slice(bytes);
    }
}

fn wait_for_child(
    child: &mut Child,
    signals: &Receiver<ProcessSignal>,
) -> io::Result<Option<ChildExitStatus>> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        match signals.recv_timeout(PROCESS_POLL_INTERVAL) {
            Ok(ProcessSignal::Cancel | ProcessSignal::PipeFailed)
            | Err(RecvTimeoutError::Disconnected) => {
                terminate_and_reap(child)?;
                return Ok(None);
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn terminate_and_reap(child: &mut Child) -> io::Result<()> {
    let kill_error = child.kill().err();
    let wait_result = child.wait();
    match (kill_error, wait_result) {
        (None, Ok(_status)) => Ok(()),
        (None, Err(wait_error)) => Err(wait_error),
        (Some(kill_error), Ok(_status)) => Err(kill_error),
        (Some(kill_error), Err(wait_error)) => Err(io::Error::other(format!(
            "failed to terminate child ({kill_error}) and then failed to reap it ({wait_error})"
        ))),
    }
}
