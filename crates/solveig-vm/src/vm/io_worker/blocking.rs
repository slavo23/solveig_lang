//! Bounded blocking-pool execution of owned host requests.
//! Each worker owns its file and HTTP handles and returns only plain Rust results.

use super::*;

pub(super) fn worker_loop(
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
            IoWork::CloseHttp(handle) => {
                // Cancellation before acquisition was observed has no Solveig
                // owner to run the bracket release. Drop its body on this worker.
                let _cleanup = handles.perform(HandleRequest::Close(handle), stdin, stdout, stderr);
                continue;
            }
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
        IoRequest::SecureRandomBytes(count) => perform_secure_random_bytes(count),
        IoRequest::ResolveHost(host) => perform_resolve_host(host),
        IoRequest::Http(request) => match super::http::perform(http_agent, request) {
            Ok(response) => IoResult::Http(response),
            Err(failure) => IoResult::Failed(IoFailure::Http(failure)),
        },
        IoRequest::OpenHttp { handle, request } => {
            let maximum = request.maximum_body;
            match super::http::open_response(http_agent, request) {
                Ok(response) => {
                    handles.insert_http_body(handle, response.body, maximum);
                    IoResult::HttpOpened(HttpResponse {
                        status: response.status,
                        headers: response.headers,
                        body: handle,
                    })
                }
                Err(failure) => IoResult::Failed(IoFailure::Http(failure)),
            }
        }
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
        | IoRequest::CloseProcess(_)
        | IoRequest::StartHttpUpload { .. }
        | IoRequest::ContinueHttpUpload { .. }
        | IoRequest::CloseHttpUpload(_) => {
            solveig_base::ice!("direct IO request reached the blocking worker pool")
        }
    }
}

fn perform_secure_random_bytes(count: i64) -> IoResult {
    let Ok(count) = usize::try_from(count) else {
        return IoResult::Failed(IoFailure::SecureRandom(io::Error::new(
            io::ErrorKind::InvalidInput,
            "byte count must be non-negative and fit the host address space",
        )));
    };
    let mut bytes = Vec::new();
    if let Err(error) = bytes.try_reserve_exact(count) {
        return IoResult::Failed(IoFailure::SecureRandom(io::Error::new(
            io::ErrorKind::OutOfMemory,
            error,
        )));
    }
    bytes.resize(count, 0);

    match getrandom::fill(&mut bytes) {
        Ok(()) => IoResult::Bytes(bytes),
        Err(error) => IoResult::Failed(IoFailure::SecureRandom(error.into())),
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
