//! Strict child-process capture on the bounded blocking worker pool.

use super::*;
use std::process::{Command, Stdio};

pub(crate) struct ProcessRequest {
    pub program: String,
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub cwd: PathBuf,
    pub environment: Vec<(String, String)>,
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

pub(super) fn perform(request: ProcessRequest) -> IoResult {
    let ProcessRequest {
        program,
        executable,
        arguments,
        cwd,
        environment,
    } = request;
    let output = Command::new(executable)
        .args(arguments)
        .current_dir(cwd)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
        .output();

    match output.and_then(decode_output) {
        Ok(output) => IoResult::Process(output),
        Err(source) => IoResult::Failed(IoFailure::Process(ProcessFailure { program, source })),
    }
}

fn decode_output(output: std::process::Output) -> io::Result<ProcessOutput> {
    let stdout = String::from_utf8(output.stdout).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("child standard output is not valid UTF-8: {error}"),
        )
    })?;
    let stderr = String::from_utf8(output.stderr).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("child standard error is not valid UTF-8: {error}"),
        )
    })?;
    Ok(ProcessOutput {
        exit_code: output.status.code().map_or(-1, i64::from),
        stdout,
        stderr,
    })
}
