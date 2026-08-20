//! Cancellable strict child-process capture on the bounded blocking worker pool.

use super::*;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(1);
const CAPTURE_CHUNK_BYTES: usize = 8 * 1024;

pub(crate) struct ProcessRequest {
    pub program: String,
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub cwd: PathBuf,
    pub environment: Vec<(String, String)>,
    pub maximum: Option<i64>,
}

pub(crate) struct ProcessOutput {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) struct ProcessFailure {
    pub program: String,
    pub source: io::Error,
}

pub(super) enum ProcessSignal {
    Cancel,
    CaptureFailed,
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
        Ok(ProcessSignal::CaptureFailed) => {
            solveig_base::ice!("process capture failed before its pipe readers started")
        }
        Err(mpsc::TryRecvError::Empty) => {}
    }

    let mut child = Command::new(executable)
        .args(arguments)
        .current_dir(cwd)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
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
        let stdout_reader =
            scope.spawn(move || read_capture(stdout, maximum, "standard output", &stdout_signals));
        let stderr_reader =
            scope.spawn(move || read_capture(stderr, maximum, "standard error", &signal_tx));

        let status = wait_for_child(&mut child, signals);
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
        decode_output(status, stdout, stderr)
    })
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
    stream_name: &'static str,
    signals: &Sender<ProcessSignal>,
) -> io::Result<Vec<u8>> {
    let result = read_capture_inner(&mut stream, maximum, stream_name);
    if result.is_err() {
        drop(signals.send(ProcessSignal::CaptureFailed));
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
) -> io::Result<Option<ExitStatus>> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        match signals.recv_timeout(PROCESS_POLL_INTERVAL) {
            Ok(ProcessSignal::Cancel | ProcessSignal::CaptureFailed)
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

fn decode_output(
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
) -> io::Result<ProcessOutput> {
    let stdout = String::from_utf8(stdout).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("child standard output is not valid UTF-8: {error}"),
        )
    })?;
    let stderr = String::from_utf8(stderr).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("child standard error is not valid UTF-8: {error}"),
        )
    })?;
    Ok(ProcessOutput {
        exit_code: status.code().map_or(-1, i64::from),
        stdout,
        stderr,
    })
}
