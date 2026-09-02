//! Host file, HTTP body, and standard-stream resources behind opaque handles.
//! Each table belongs to one blocking worker, so requests for one handle stay
//! FIFO while separate workers can make progress on separate handles.

use std::fs::File;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;

mod file;
mod http;

use file::open_host_file;
pub(crate) use http::HttpBody;
pub(crate) use http::normalize_http_read_error;

const DEFAULT_BLOCK_SIZE: usize = 8 * 1024;

pub(crate) const STDIN_HANDLE: HandleId = HandleId(0);
pub(crate) const STDOUT_HANDLE: HandleId = HandleId(1);
pub(crate) const STDERR_HANDLE: HandleId = HandleId(2);
pub(crate) const FIRST_FILE_HANDLE: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HandleId(pub(crate) u32);

#[derive(Clone, Copy, Debug)]
pub enum FileMode {
    Read,
    Write,
    Append,
    ReadWrite,
}

#[derive(Clone, Copy, Debug)]
pub enum Buffering {
    None,
    Line,
    Block(Option<i64>),
}

impl Buffering {
    fn capacity(self) -> io::Result<usize> {
        match self {
            Self::None => Ok(1),
            Self::Line | Self::Block(None) => Ok(DEFAULT_BLOCK_SIZE),
            Self::Block(Some(size)) => usize::try_from(size)
                .ok()
                .filter(|size| *size > 0)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "block buffer size must be a positive Int",
                    )
                }),
        }
    }

    fn flushes(self, pending: &[u8]) -> io::Result<bool> {
        Ok(match self {
            Self::None => true,
            Self::Line => pending.contains(&b'\n'),
            Self::Block(_) => pending.len() >= self.capacity()?,
        })
    }
}

pub(crate) enum HandleRequest {
    OpenFile {
        handle: HandleId,
        path: PathBuf,
        mode: FileMode,
    },
    Close(HandleId),
    Flush(HandleId),
    GetLine(HandleId),
    GetContents(HandleId),
    GetBytes {
        handle: HandleId,
        maximum: i64,
    },
    Put {
        handle: HandleId,
        text: String,
        newline: bool,
    },
    PutBytes {
        handle: HandleId,
        bytes: Vec<u8>,
    },
    IsEof(HandleId),
    SetBuffering {
        handle: HandleId,
        buffering: Buffering,
    },
}

impl HandleRequest {
    pub(crate) fn handle(&self) -> HandleId {
        match self {
            Self::OpenFile { handle, .. }
            | Self::Close(handle)
            | Self::Flush(handle)
            | Self::GetLine(handle)
            | Self::GetContents(handle)
            | Self::GetBytes { handle, .. }
            | Self::Put { handle, .. }
            | Self::PutBytes { handle, .. }
            | Self::IsEof(handle)
            | Self::SetBuffering { handle, .. } => *handle,
        }
    }
}

pub(crate) enum HandleOutput {
    Unit,
    Opened(HandleId),
    Text(String),
    Bytes(Vec<u8>),
    Bool(bool),
}

pub(crate) enum HandleFailure {
    Open { path: PathBuf, source: io::Error },
    Close { label: String, source: io::Error },
    Flush { label: String, source: io::Error },
    Read { label: String, source: io::Error },
    Write { label: String, source: io::Error },
    SetBuffering { label: String, source: io::Error },
    HttpBodyTooLarge { maximum: usize },
}

enum HandleSlot {
    Vacant,
    Open(HostHandle),
    Closed { label: String },
}

enum HostHandle {
    Input {
        label: String,
        buffering: Buffering,
    },
    Output {
        label: String,
        stream: StandardOutput,
        buffering: Buffering,
        pending: Vec<u8>,
    },
    File(HostFile),
    Http(HttpBody),
}

#[derive(Clone, Copy)]
enum StandardOutput {
    Stdout,
    Stderr,
}

struct HostFile {
    path: PathBuf,
    file: File,
    mode: FileMode,
    buffering: Buffering,
    read_buffer: Vec<u8>,
    read_cursor: usize,
    write_buffer: Vec<u8>,
}

pub(crate) struct HandleTable {
    slots: Vec<HandleSlot>,
}

impl HandleTable {
    pub(crate) fn insert_http_body(
        &mut self,
        handle: HandleId,
        body: ureq::Body,
        maximum: Option<usize>,
    ) {
        *self.slot_mut(handle) = HandleSlot::Open(HostHandle::Http(HttpBody::new(body, maximum)));
    }

    pub(crate) fn for_worker(worker: usize, terminal_outputs: (bool, bool)) -> Self {
        let mut slots = vec![HandleSlot::Vacant, HandleSlot::Vacant, HandleSlot::Vacant];
        match worker {
            0 => {
                let Some(slot) = slots.get_mut(STDIN_HANDLE.0 as usize) else {
                    solveig_base::ice!("stdin handle identity exceeded standard handle table")
                };
                *slot = HandleSlot::Open(HostHandle::Input {
                    label: "stdin".to_string(),
                    buffering: Buffering::Line,
                });
            }
            1 => {
                let Some(slot) = slots.get_mut(STDOUT_HANDLE.0 as usize) else {
                    solveig_base::ice!("stdout handle identity exceeded standard handle table")
                };
                *slot = HandleSlot::Open(HostHandle::Output {
                    label: "stdout".to_string(),
                    stream: StandardOutput::Stdout,
                    buffering: if terminal_outputs.0 {
                        Buffering::Line
                    } else {
                        Buffering::Block(None)
                    },
                    pending: Vec::new(),
                });
            }
            2 => {
                let Some(slot) = slots.get_mut(STDERR_HANDLE.0 as usize) else {
                    solveig_base::ice!("stderr handle identity exceeded standard handle table")
                };
                *slot = HandleSlot::Open(HostHandle::Output {
                    label: "stderr".to_string(),
                    stream: StandardOutput::Stderr,
                    buffering: if terminal_outputs.1 {
                        Buffering::Line
                    } else {
                        Buffering::Block(None)
                    },
                    pending: Vec::new(),
                });
            }
            _ => {}
        }
        Self { slots }
    }

    pub(crate) fn perform(
        &mut self,
        request: HandleRequest,
        stdin: &Mutex<&mut (dyn BufRead + Send)>,
        stdout: &Mutex<&mut (dyn Write + Send)>,
        stderr: &Mutex<&mut (dyn Write + Send)>,
    ) -> Result<HandleOutput, HandleFailure> {
        match request {
            HandleRequest::OpenFile { handle, path, mode } => self.open_file(handle, path, mode),
            HandleRequest::Close(handle) => self.close(handle, stdout, stderr),
            HandleRequest::Flush(handle) => self.flush(handle, stdout, stderr),
            HandleRequest::GetLine(handle) => self.get_line(handle, stdin),
            HandleRequest::GetContents(handle) => self.get_contents(handle, stdin),
            HandleRequest::GetBytes { handle, maximum } => self.get_bytes(handle, maximum, stdin),
            HandleRequest::Put {
                handle,
                mut text,
                newline,
            } => {
                if newline {
                    text.push('\n');
                }
                self.put(handle, text.as_bytes(), stdout, stderr)
            }
            HandleRequest::PutBytes { handle, bytes } => self.put(handle, &bytes, stdout, stderr),
            HandleRequest::IsEof(handle) => self.is_eof(handle, stdin),
            HandleRequest::SetBuffering { handle, buffering } => {
                self.set_buffering(handle, buffering, stdout, stderr)
            }
        }
    }

    pub(crate) fn shutdown(
        &mut self,
        stdout: &Mutex<&mut (dyn Write + Send)>,
        stderr: &Mutex<&mut (dyn Write + Send)>,
    ) {
        for slot in &mut self.slots {
            if let HandleSlot::Open(host) = slot {
                let _ignored_shutdown_error = flush_host(host, stdout, stderr);
            }
        }
    }

    fn open_file(
        &mut self,
        handle: HandleId,
        path: PathBuf,
        mode: FileMode,
    ) -> Result<HandleOutput, HandleFailure> {
        let file = open_host_file(&path, mode).map_err(|source| HandleFailure::Open {
            path: path.clone(),
            source,
        })?;
        let host = HostFile {
            path,
            file,
            mode,
            buffering: Buffering::Block(None),
            read_buffer: Vec::new(),
            read_cursor: 0,
            write_buffer: Vec::new(),
        };
        let index = handle.0 as usize;
        if self.slots.len() <= index {
            self.slots.resize_with(index + 1, || HandleSlot::Vacant);
        }
        let Some(slot) = self.slots.get_mut(index) else {
            solveig_base::ice!("new file handle slot disappeared after table growth")
        };
        *slot = HandleSlot::Open(HostHandle::File(host));
        Ok(HandleOutput::Opened(handle))
    }

    fn close(
        &mut self,
        handle: HandleId,
        stdout: &Mutex<&mut (dyn Write + Send)>,
        stderr: &Mutex<&mut (dyn Write + Send)>,
    ) -> Result<HandleOutput, HandleFailure> {
        let slot = self.slot_mut(handle);
        let label = slot_label(slot, handle);
        let result = match slot {
            HandleSlot::Open(host) => flush_host(host, stdout, stderr),
            HandleSlot::Closed { .. } | HandleSlot::Vacant => Err(closed_error(&label)),
        };
        *slot = HandleSlot::Closed {
            label: label.clone(),
        };
        result
            .map(|()| HandleOutput::Unit)
            .map_err(|source| HandleFailure::Close { label, source })
    }

    fn flush(
        &mut self,
        handle: HandleId,
        stdout: &Mutex<&mut (dyn Write + Send)>,
        stderr: &Mutex<&mut (dyn Write + Send)>,
    ) -> Result<HandleOutput, HandleFailure> {
        let slot = self.slot_mut(handle);
        let label = slot_label(slot, handle);
        match slot {
            HandleSlot::Open(host) => flush_host(host, stdout, stderr),
            HandleSlot::Closed { .. } | HandleSlot::Vacant => Err(closed_error(&label)),
        }
        .map(|()| HandleOutput::Unit)
        .map_err(|source| HandleFailure::Flush { label, source })
    }

    fn get_line(
        &mut self,
        handle: HandleId,
        stdin: &Mutex<&mut (dyn BufRead + Send)>,
    ) -> Result<HandleOutput, HandleFailure> {
        let slot = self.slot_mut(handle);
        let label = slot_label(slot, handle);
        let result = match slot {
            HandleSlot::Open(HostHandle::Input { .. }) => read_standard_line(stdin),
            HandleSlot::Open(HostHandle::File(file)) => file.read_line(),
            HandleSlot::Open(HostHandle::Http(_)) => Err(http_bytes_only()),
            HandleSlot::Open(HostHandle::Output { .. }) => Err(not_readable_error()),
            HandleSlot::Closed { .. } | HandleSlot::Vacant => Err(closed_error(&label)),
        };
        result
            .map(HandleOutput::Text)
            .map_err(|source| HandleFailure::Read { label, source })
    }

    fn get_contents(
        &mut self,
        handle: HandleId,
        stdin: &Mutex<&mut (dyn BufRead + Send)>,
    ) -> Result<HandleOutput, HandleFailure> {
        let slot = self.slot_mut(handle);
        let label = slot_label(slot, handle);
        let result = match slot {
            HandleSlot::Open(HostHandle::Input { .. }) => read_standard_contents(stdin),
            HandleSlot::Open(HostHandle::File(file)) => file.read_contents(),
            HandleSlot::Open(HostHandle::Http(_)) => Err(http_bytes_only()),
            HandleSlot::Open(HostHandle::Output { .. }) => Err(not_readable_error()),
            HandleSlot::Closed { .. } | HandleSlot::Vacant => Err(closed_error(&label)),
        };
        result
            .map(HandleOutput::Text)
            .map_err(|source| HandleFailure::Read { label, source })
    }

    fn get_bytes(
        &mut self,
        handle: HandleId,
        maximum: i64,
        stdin: &Mutex<&mut (dyn BufRead + Send)>,
    ) -> Result<HandleOutput, HandleFailure> {
        let slot = self.slot_mut(handle);
        let label = slot_label(slot, handle);
        let result = match slot {
            HandleSlot::Open(HostHandle::Input { .. }) => read_standard_bytes(stdin, maximum),
            HandleSlot::Open(HostHandle::File(file)) => file.read_bytes(maximum),
            HandleSlot::Open(HostHandle::Http(body)) => {
                return body.read_http_chunk(maximum).map(HandleOutput::Bytes);
            }
            HandleSlot::Open(HostHandle::Output { .. }) => Err(not_readable_error()),
            HandleSlot::Closed { .. } | HandleSlot::Vacant => Err(closed_error(&label)),
        };
        result
            .map(HandleOutput::Bytes)
            .map_err(|source| HandleFailure::Read { label, source })
    }

    fn put(
        &mut self,
        handle: HandleId,
        bytes: &[u8],
        stdout: &Mutex<&mut (dyn Write + Send)>,
        stderr: &Mutex<&mut (dyn Write + Send)>,
    ) -> Result<HandleOutput, HandleFailure> {
        let slot = self.slot_mut(handle);
        let label = slot_label(slot, handle);
        let result = match slot {
            HandleSlot::Open(HostHandle::Output {
                stream,
                buffering,
                pending,
                ..
            }) => write_standard(*stream, *buffering, pending, bytes, stdout, stderr),
            HandleSlot::Open(HostHandle::File(file)) => file.write(bytes),
            HandleSlot::Open(HostHandle::Input { .. } | HostHandle::Http(_)) => {
                Err(not_writable_error())
            }
            HandleSlot::Closed { .. } | HandleSlot::Vacant => Err(closed_error(&label)),
        };
        result
            .map(|()| HandleOutput::Unit)
            .map_err(|source| HandleFailure::Write { label, source })
    }

    fn is_eof(
        &mut self,
        handle: HandleId,
        stdin: &Mutex<&mut (dyn BufRead + Send)>,
    ) -> Result<HandleOutput, HandleFailure> {
        let slot = self.slot_mut(handle);
        let label = slot_label(slot, handle);
        let result = match slot {
            HandleSlot::Open(HostHandle::Input { .. }) => standard_is_eof(stdin),
            HandleSlot::Open(HostHandle::File(file)) => file.is_eof(),
            HandleSlot::Open(HostHandle::Http(_)) => Err(http_bytes_only()),
            HandleSlot::Open(HostHandle::Output { .. }) => Err(not_readable_error()),
            HandleSlot::Closed { .. } | HandleSlot::Vacant => Err(closed_error(&label)),
        };
        result
            .map(HandleOutput::Bool)
            .map_err(|source| HandleFailure::Read { label, source })
    }

    fn set_buffering(
        &mut self,
        handle: HandleId,
        buffering: Buffering,
        stdout: &Mutex<&mut (dyn Write + Send)>,
        stderr: &Mutex<&mut (dyn Write + Send)>,
    ) -> Result<HandleOutput, HandleFailure> {
        let slot = self.slot_mut(handle);
        let label = slot_label(slot, handle);
        let result = buffering.capacity().and_then(|_| match slot {
            HandleSlot::Open(host) => {
                flush_host(host, stdout, stderr)?;
                match host {
                    HostHandle::Input {
                        buffering: current, ..
                    }
                    | HostHandle::Output {
                        buffering: current, ..
                    } => *current = buffering,
                    HostHandle::File(file) => file.set_buffering(buffering)?,
                    HostHandle::Http(_) => return Err(http_bytes_only()),
                }
                Ok(())
            }
            HandleSlot::Closed { .. } | HandleSlot::Vacant => Err(closed_error(&label)),
        });
        result
            .map(|()| HandleOutput::Unit)
            .map_err(|source| HandleFailure::SetBuffering { label, source })
    }

    fn slot_mut(&mut self, handle: HandleId) -> &mut HandleSlot {
        let index = handle.0 as usize;
        if self.slots.len() <= index {
            self.slots.resize_with(index + 1, || HandleSlot::Vacant);
        }
        let Some(slot) = self.slots.get_mut(index) else {
            solveig_base::ice!("handle slot disappeared after table growth")
        };
        slot
    }
}

fn slot_label(slot: &HandleSlot, handle: HandleId) -> String {
    match slot {
        HandleSlot::Open(HostHandle::Input { label, .. })
        | HandleSlot::Open(HostHandle::Output { label, .. })
        | HandleSlot::Closed { label } => label.clone(),
        HandleSlot::Open(HostHandle::File(file)) => file.path.display().to_string(),
        HandleSlot::Open(HostHandle::Http(_)) => "HTTP response body".to_string(),
        HandleSlot::Vacant => format!("handle {}", handle.0),
    }
}

fn flush_host(
    host: &mut HostHandle,
    stdout: &Mutex<&mut (dyn Write + Send)>,
    stderr: &Mutex<&mut (dyn Write + Send)>,
) -> io::Result<()> {
    match host {
        HostHandle::Input { .. } | HostHandle::Http(_) => Ok(()),
        HostHandle::Output {
            stream, pending, ..
        } => flush_standard(*stream, pending, stdout, stderr),
        HostHandle::File(file) => file.flush(),
    }
}

fn http_bytes_only() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "HTTP bodies support bounded byte reads and close",
    )
}

fn read_standard_line(stdin: &Mutex<&mut (dyn BufRead + Send)>) -> io::Result<String> {
    let mut stdin = stdin
        .lock()
        .map_err(|_poisoned| io::Error::other("stdin lock was poisoned"))?;
    let mut line = String::new();
    stdin.read_line(&mut line)?;
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    Ok(line)
}

fn read_standard_contents(stdin: &Mutex<&mut (dyn BufRead + Send)>) -> io::Result<String> {
    let mut stdin = stdin
        .lock()
        .map_err(|_poisoned| io::Error::other("stdin lock was poisoned"))?;
    let mut contents = String::new();
    stdin.read_to_string(&mut contents)?;
    Ok(contents)
}

fn read_standard_bytes(
    stdin: &Mutex<&mut (dyn BufRead + Send)>,
    maximum: i64,
) -> io::Result<Vec<u8>> {
    let maximum = usize::try_from(maximum).map_err(|_negative| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "hGetBytes maximum must be non-negative and fit the host address space",
        )
    })?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(maximum).map_err(|error| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            format!("byte read is too large: {error}"),
        )
    })?;
    bytes.resize(maximum, 0);
    let mut stdin = stdin
        .lock()
        .map_err(|_poisoned| io::Error::other("stdin lock was poisoned"))?;
    let read = stdin.read(&mut bytes)?;
    bytes.truncate(read);
    Ok(bytes)
}

fn standard_is_eof(stdin: &Mutex<&mut (dyn BufRead + Send)>) -> io::Result<bool> {
    let mut stdin = stdin
        .lock()
        .map_err(|_poisoned| io::Error::other("stdin lock was poisoned"))?;
    Ok(stdin.fill_buf()?.is_empty())
}

fn write_standard(
    stream: StandardOutput,
    buffering: Buffering,
    pending: &mut Vec<u8>,
    bytes: &[u8],
    stdout: &Mutex<&mut (dyn Write + Send)>,
    stderr: &Mutex<&mut (dyn Write + Send)>,
) -> io::Result<()> {
    pending.extend_from_slice(bytes);
    if buffering.flushes(pending)? {
        flush_standard(stream, pending, stdout, stderr)?;
    }
    Ok(())
}

fn flush_standard(
    stream: StandardOutput,
    pending: &mut Vec<u8>,
    stdout: &Mutex<&mut (dyn Write + Send)>,
    stderr: &Mutex<&mut (dyn Write + Send)>,
) -> io::Result<()> {
    match stream {
        StandardOutput::Stdout => flush_locked_standard(pending, stdout),
        StandardOutput::Stderr => flush_locked_standard(pending, stderr),
    }
}

fn flush_locked_standard(
    pending: &mut Vec<u8>,
    target: &Mutex<&mut (dyn Write + Send)>,
) -> io::Result<()> {
    let mut target = target
        .lock()
        .map_err(|_poisoned| io::Error::other("standard stream lock was poisoned"))?;
    target.write_all(pending)?;
    pending.clear();
    target.flush()
}

fn closed_error(label: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotConnected, format!("{label} is closed"))
}

fn not_readable_error() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "handle is not readable")
}

fn not_writable_error() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "handle is not writable")
}
