//! One-shot scoped HTTP uploads driven by Solveig stream demand.
//!
//! A joined host thread owns ureq's synchronous request and response body. A
//! one-slot command channel transfers at most one supplied chunk, while each
//! reader demand completes exactly one VM IO request. The same resource owns
//! the eventual response handle, so no response body crosses worker ownership.

use super::*;
use crate::handle::{HandleFailure, HttpBody};
use std::sync::{Arc, Mutex, mpsc::SyncSender};
use std::thread::JoinHandle;

enum UploadCommand {
    Supply {
        id: RequestId,
        bytes: Option<Vec<u8>>,
    },
    Read {
        id: RequestId,
        maximum: i64,
    },
    Close(RequestId),
    Cancel(RequestId),
    Shutdown,
}

struct UploadControl {
    current: RequestId,
    closing: Option<RequestId>,
    shutdown: bool,
}

pub(in crate::vm) struct StreamingHttpUpload {
    completions: mpsc::Sender<IoCompletion>,
    commands: SyncSender<UploadCommand>,
    thread: Option<JoinHandle<()>>,
}

impl StreamingHttpUpload {
    pub(super) fn supply(&self, id: RequestId, bytes: Option<Vec<u8>>) -> io::Result<()> {
        self.send(UploadCommand::Supply { id, bytes })
    }

    pub(super) fn read(&self, id: RequestId, maximum: i64) -> io::Result<()> {
        self.send(UploadCommand::Read { id, maximum })
    }

    pub(super) fn close(&self, id: RequestId) -> io::Result<()> {
        match self.commands.send(UploadCommand::Close(id)) {
            Ok(()) => Ok(()),
            Err(_disconnected) => self
                .completions
                .send(IoCompletion {
                    id,
                    result: IoResult::Unit,
                })
                .map_err(|_disconnected| closed_upload()),
        }
    }

    pub(super) fn cancel(&self, id: RequestId) {
        drop(self.commands.send(UploadCommand::Cancel(id)));
    }

    pub(super) fn shutdown_and_join(mut self) {
        drop(self.commands.send(UploadCommand::Shutdown));
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }

    pub(super) fn join(mut self) -> Result<(), RuntimeError> {
        let Some(thread) = self.thread.take() else {
            solveig_base::ice!("streaming HTTP upload was joined more than once")
        };
        thread.join().map_err(|_panic| {
            RuntimeError::builtin("streaming HTTP upload panicked during cleanup".to_string())
        })
    }

    fn send(&self, command: UploadCommand) -> io::Result<()> {
        self.commands
            .send(command)
            .map_err(|_disconnected| closed_upload())
    }
}

pub(super) fn start(
    handle: HandleId,
    mut request: HttpRequest,
    start_id: RequestId,
    completions: mpsc::Sender<IoCompletion>,
) -> io::Result<StreamingHttpUpload> {
    let maximum = request.maximum_body;
    let (commands, command_rx) = mpsc::sync_channel(1);
    let control = Arc::new(Mutex::new(UploadControl {
        current: start_id,
        closing: None,
        shutdown: false,
    }));
    let shared_commands = Arc::new(Mutex::new(command_rx));
    request.body = super::super::http::HttpRequestBody::Stream(Box::new(DemandReader {
        handle,
        completions: completions.clone(),
        commands: shared_commands.clone(),
        control: control.clone(),
        current: io::Cursor::new(Vec::new()),
        ended: false,
    }));
    let thread_completions = completions.clone();
    let commands_for_response = shared_commands;
    let thread = thread::Builder::new()
        .name("solveig-http-upload".to_string())
        .spawn(move || {
            let result = super::super::http::open_response(&super::super::http::agent(), request);
            let state = control.lock().expect("HTTP upload control poisoned");
            let current = state.current;
            let closing = state.closing;
            let shutdown = state.shutdown;
            drop(state);
            if shutdown {
                return;
            }
            if let Some(id) = closing {
                drop(thread_completions.send(IoCompletion {
                    id,
                    result: IoResult::Unit,
                }));
                return;
            }
            let response = match result {
                Ok(response) => response,
                Err(failure) => {
                    drop(thread_completions.send(IoCompletion {
                        id: current,
                        result: IoResult::Failed(IoFailure::Http(failure)),
                    }));
                    return;
                }
            };
            let status = response.status;
            let headers = response.headers;
            let mut body = HttpBody::new(response.body, maximum);
            if thread_completions
                .send(IoCompletion {
                    id: current,
                    result: IoResult::HttpUploadOpened(HttpResponse {
                        status,
                        headers,
                        body: handle,
                    }),
                })
                .is_err()
            {
                return;
            }
            response_loop(&commands_for_response, &thread_completions, &mut body);
        })?;
    Ok(StreamingHttpUpload {
        completions,
        commands,
        thread: Some(thread),
    })
}

struct DemandReader {
    handle: HandleId,
    completions: mpsc::Sender<IoCompletion>,
    commands: Arc<Mutex<mpsc::Receiver<UploadCommand>>>,
    control: Arc<Mutex<UploadControl>>,
    current: io::Cursor<Vec<u8>>,
    ended: bool,
}

impl Read for DemandReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        loop {
            let count = self.current.read(output)?;
            if count != 0 || self.ended {
                return Ok(count);
            }
            let id = self
                .control
                .lock()
                .map_err(|_poisoned| closed_upload())?
                .current;
            self.completions
                .send(IoCompletion {
                    id,
                    result: IoResult::HttpUploadDemand(self.handle),
                })
                .map_err(|_disconnected| closed_upload())?;
            let command = self
                .commands
                .lock()
                .map_err(|_poisoned| closed_upload())?
                .recv()
                .map_err(|_disconnected| closed_upload())?;
            match command {
                UploadCommand::Supply { id, bytes } => {
                    self.control
                        .lock()
                        .map_err(|_poisoned| closed_upload())?
                        .current = id;
                    match bytes {
                        Some(bytes) => self.current = io::Cursor::new(bytes),
                        None => self.ended = true,
                    }
                }
                UploadCommand::Close(id) => {
                    self.control
                        .lock()
                        .map_err(|_poisoned| closed_upload())?
                        .closing = Some(id);
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "upload closed"));
                }
                UploadCommand::Cancel(id) => {
                    self.control
                        .lock()
                        .map_err(|_poisoned| closed_upload())?
                        .current = id;
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "upload cancelled",
                    ));
                }
                UploadCommand::Shutdown => {
                    self.control
                        .lock()
                        .map_err(|_poisoned| closed_upload())?
                        .shutdown = true;
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "upload shutdown",
                    ));
                }
                UploadCommand::Read { .. } => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "response read issued before upload completed",
                    ));
                }
            }
        }
    }
}

fn response_loop(
    commands: &Mutex<mpsc::Receiver<UploadCommand>>,
    completions: &mpsc::Sender<IoCompletion>,
    body: &mut HttpBody,
) {
    loop {
        let command = match commands.lock() {
            Ok(commands) => commands.recv(),
            Err(_poisoned) => return,
        };
        match command {
            Ok(UploadCommand::Read { id, maximum }) => {
                let result = match body.read_http_chunk(maximum) {
                    Ok(bytes) => IoResult::Bytes(bytes),
                    Err(failure) => IoResult::Failed(IoFailure::Handle(failure)),
                };
                if completions.send(IoCompletion { id, result }).is_err() {
                    return;
                }
            }
            Ok(UploadCommand::Close(id)) => {
                drop(completions.send(IoCompletion {
                    id,
                    result: IoResult::Unit,
                }));
                return;
            }
            Ok(UploadCommand::Cancel(id)) => {
                drop(completions.send(IoCompletion {
                    id,
                    result: IoResult::Failed(IoFailure::Handle(HandleFailure::Read {
                        label: "HTTP response body".to_string(),
                        source: io::Error::new(io::ErrorKind::Interrupted, "upload cancelled"),
                    })),
                }));
                return;
            }
            Ok(UploadCommand::Shutdown) | Err(_) => return,
            Ok(UploadCommand::Supply { id, .. }) => {
                drop(completions.send(IoCompletion {
                    id,
                    result: IoResult::Failed(IoFailure::Handle(HandleFailure::Read {
                        label: "HTTP upload".to_string(),
                        source: io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "upload chunk supplied after response opened",
                        ),
                    })),
                }));
            }
        }
    }
}

fn closed_upload() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "HTTP upload resource is closed")
}
