//! Readiness-driven TCP resources for the VM IO subsystem.
//! One event-loop thread owns every OS socket, while the scheduler exchanges
//! typed ids, owned byte buffers, and ordinary IO completions with it.

use super::io_worker::{IoCompletion, IoFailure, IoResult};
use super::*;
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token, Waker};
use std::collections::VecDeque;
use std::io;
use std::sync::mpsc;
use std::thread;

mod state;

use state::*;

const WAKE_TOKEN: Token = Token(0);
const EVENT_CAPACITY: usize = 128;

pub(super) enum SocketRequest {
    Connect { host: String, port: i64 },
    Listen { port: i64 },
    Accept(SocketId),
    Send { socket: SocketId, bytes: Vec<u8> },
    Recv { socket: SocketId, maximum: i64 },
    Close(SocketId),
}

enum SocketCommand {
    Issue {
        id: RequestId,
        request: SocketRequest,
    },
    Cancel(RequestId),
    Shutdown,
}

pub(super) struct SocketSubsystem {
    commands: Option<mpsc::Sender<SocketCommand>>,
    waker: Option<Waker>,
}

impl SocketSubsystem {
    pub(super) fn inactive() -> Self {
        Self {
            commands: None,
            waker: None,
        }
    }

    pub(super) fn start<'scope>(
        scope: &'scope thread::Scope<'scope, '_>,
        completions: mpsc::Sender<IoCompletion>,
    ) -> io::Result<Self> {
        let poll = Poll::new()?;
        let waker = Waker::new(poll.registry(), WAKE_TOKEN)?;
        let (command_tx, command_rx) = mpsc::channel();
        let handle = thread::Builder::new()
            .name("solveig-sockets".to_string())
            .spawn_scoped(scope, move || {
                SocketLoop::new(poll, command_rx, completions).run();
            })?;
        drop(handle);
        Ok(Self {
            commands: Some(command_tx),
            waker: Some(waker),
        })
    }

    pub(super) fn issue(&self, id: RequestId, request: SocketRequest) -> Result<(), RuntimeError> {
        let Some(commands) = &self.commands else {
            return Err(RuntimeError::builtin(
                "socket request issued without an active event loop".to_string(),
            ));
        };
        commands
            .send(SocketCommand::Issue { id, request })
            .map_err(|_disconnected| {
                RuntimeError::builtin("socket event loop stopped".to_string())
            })?;
        self.wake()
    }

    pub(super) fn cancel(&self, id: RequestId) -> Result<(), RuntimeError> {
        let Some(commands) = &self.commands else {
            return Ok(());
        };
        commands
            .send(SocketCommand::Cancel(id))
            .map_err(|_disconnected| {
                RuntimeError::builtin("socket event loop stopped".to_string())
            })?;
        self.wake()
    }

    fn wake(&self) -> Result<(), RuntimeError> {
        let Some(waker) = &self.waker else {
            return Ok(());
        };
        waker
            .wake()
            .map_err(|error| RuntimeError::builtin(format!("failed to wake socket loop: {error}")))
    }
}

impl Drop for SocketSubsystem {
    fn drop(&mut self) {
        if let Some(commands) = self.commands.take() {
            drop(commands.send(SocketCommand::Shutdown));
        }
        if let Some(waker) = self.waker.take() {
            drop(waker.wake());
        }
    }
}

struct SocketLoop {
    poll: Poll,
    events: Events,
    commands: mpsc::Receiver<SocketCommand>,
    completions: mpsc::Sender<IoCompletion>,
    sockets: Vec<Option<SocketEntry>>,
}

impl SocketLoop {
    fn new(
        poll: Poll,
        commands: mpsc::Receiver<SocketCommand>,
        completions: mpsc::Sender<IoCompletion>,
    ) -> Self {
        Self {
            poll,
            events: Events::with_capacity(EVENT_CAPACITY),
            commands,
            completions,
            sockets: Vec::new(),
        }
    }

    fn run(mut self) {
        loop {
            if let Err(error) = self.poll.poll(&mut self.events, None) {
                let message = format!("socket event polling failed: {error}");
                self.fail_all(&message);
                return;
            }
            let events = self.events.iter().cloned().collect::<Vec<_>>();
            for event in &events {
                if event.token() == WAKE_TOKEN {
                    if !self.drain_commands() {
                        return;
                    }
                } else if !self.handle_event(event) {
                    return;
                }
            }
        }
    }

    fn drain_commands(&mut self) -> bool {
        loop {
            match self.commands.try_recv() {
                Ok(SocketCommand::Issue { id, request }) => self.issue(id, request),
                Ok(SocketCommand::Cancel(id)) => self.cancel(id),
                Ok(SocketCommand::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => {
                    return false;
                }
                Err(mpsc::TryRecvError::Empty) => return true,
            }
        }
    }

    fn issue(&mut self, id: RequestId, request: SocketRequest) {
        match request {
            SocketRequest::Connect { host, port } => self.connect(id, host, port),
            SocketRequest::Listen { port } => self.listen(id, port),
            SocketRequest::Accept(socket) => self.accept(id, socket),
            SocketRequest::Send { socket, bytes } => self.send(id, socket, bytes),
            SocketRequest::Recv { socket, maximum } => self.recv(id, socket, maximum),
            SocketRequest::Close(socket) => self.close(id, socket),
        }
    }

    fn connect(&mut self, id: RequestId, host: String, port: i64) {
        let address = socket_address(&host, port);
        let socket = address.and_then(TcpStream::connect);
        let mut socket = match socket {
            Ok(socket) => socket,
            Err(source) => {
                self.complete(
                    id,
                    IoResult::Failed(IoFailure::Connect { host, port, source }),
                );
                return;
            }
        };
        let socket_id = self.next_socket_id();
        if let Err(source) =
            self.poll
                .registry()
                .register(&mut socket, socket_token(socket_id), Interest::WRITABLE)
        {
            self.complete(
                id,
                IoResult::Failed(IoFailure::Connect { host, port, source }),
            );
            return;
        }
        self.insert_socket(
            socket_id,
            SocketEntry::Stream {
                socket,
                connect: Some(ConnectRequest { id, host, port }),
                reads: VecDeque::new(),
                writes: VecDeque::new(),
                registered: true,
            },
        );
    }

    fn listen(&mut self, id: RequestId, port: i64) {
        let address = loopback_address(port);
        let socket = address.and_then(TcpListener::bind);
        let socket = match socket {
            Ok(socket) => socket,
            Err(source) => {
                self.complete(id, IoResult::Failed(IoFailure::Listen { port, source }));
                return;
            }
        };
        let actual_port = match socket.local_addr() {
            Ok(address) => address.port(),
            Err(source) => {
                self.complete(id, IoResult::Failed(IoFailure::Listen { port, source }));
                return;
            }
        };
        let socket_id = self.next_socket_id();
        self.insert_socket(
            socket_id,
            SocketEntry::Listener {
                socket,
                accepts: VecDeque::new(),
                registered: false,
            },
        );
        self.complete(
            id,
            IoResult::Listener {
                socket: socket_id,
                port: actual_port,
            },
        );
    }

    fn accept(&mut self, id: RequestId, socket_id: SocketId) {
        let Some(entry) = self.socket_mut(socket_id) else {
            self.complete(
                id,
                IoResult::Failed(IoFailure::Accept(closed_socket_error())),
            );
            return;
        };
        match entry {
            SocketEntry::Listener { accepts, .. } => accepts.push_back(id),
            SocketEntry::Stream { .. } => {
                self.complete(
                    id,
                    IoResult::Failed(IoFailure::Accept(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "accept requires a listening socket",
                    ))),
                );
                return;
            }
        }
        self.refresh(socket_id);
    }

    fn send(&mut self, id: RequestId, socket_id: SocketId, bytes: Vec<u8>) {
        let Some(entry) = self.socket_mut(socket_id) else {
            self.complete(id, IoResult::Failed(IoFailure::Send(closed_socket_error())));
            return;
        };
        match entry {
            SocketEntry::Stream { .. } if bytes.is_empty() => {
                self.complete(id, IoResult::Unit);
                return;
            }
            SocketEntry::Stream { writes, .. } => {
                writes.push_back(WriteRequest {
                    id,
                    bytes,
                    written: 0,
                });
            }
            SocketEntry::Listener { .. } => {
                self.complete(
                    id,
                    IoResult::Failed(IoFailure::Send(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "sendBytes requires a connected socket",
                    ))),
                );
                return;
            }
        }
        self.refresh(socket_id);
    }

    fn recv(&mut self, id: RequestId, socket_id: SocketId, maximum: i64) {
        let maximum = match usize::try_from(maximum) {
            Ok(maximum) => maximum,
            Err(_) => {
                self.complete(
                    id,
                    IoResult::Failed(IoFailure::Recv(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "recvBytes maximum must be non-negative",
                    ))),
                );
                return;
            }
        };
        let Some(entry) = self.socket_mut(socket_id) else {
            self.complete(id, IoResult::Failed(IoFailure::Recv(closed_socket_error())));
            return;
        };
        match entry {
            SocketEntry::Stream { .. } if maximum == 0 => {
                self.complete(id, IoResult::Bytes(Vec::new()));
                return;
            }
            SocketEntry::Stream { reads, .. } => reads.push_back(ReadRequest { id, maximum }),
            SocketEntry::Listener { .. } => {
                self.complete(
                    id,
                    IoResult::Failed(IoFailure::Recv(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "recvBytes requires a connected socket",
                    ))),
                );
                return;
            }
        }
        self.refresh(socket_id);
    }

    fn close(&mut self, id: RequestId, socket_id: SocketId) {
        let Some(mut entry) = self.take_socket(socket_id) else {
            self.complete(id, IoResult::Unit);
            return;
        };
        let close_error = entry
            .is_registered()
            .then(|| entry.deregister(self.poll.registry()))
            .transpose()
            .err();
        self.fail_entry(entry, "socket closed while operation was pending");
        let result = close_error.map_or(IoResult::Unit, |error| {
            IoResult::Failed(IoFailure::CloseSocket(error))
        });
        self.complete(id, result);
    }

    fn cancel(&mut self, id: RequestId) {
        for index in 0..self.sockets.len() {
            let Ok(raw) = u32::try_from(index) else {
                solveig_base::ice!("socket table index no longer fits SocketId")
            };
            let socket_id = SocketId(raw);
            let Some(mut entry) = self.take_socket(socket_id) else {
                continue;
            };
            match entry.cancel(id) {
                CancelEffect::Absent => self.put_socket(socket_id, entry),
                CancelEffect::Keep => {
                    self.put_socket(socket_id, entry);
                    self.refresh(socket_id);
                    self.complete(id, IoResult::Unit);
                    return;
                }
                CancelEffect::Close => {
                    if entry.is_registered() {
                        drop(entry.deregister(self.poll.registry()));
                    }
                    self.complete(id, IoResult::Unit);
                    return;
                }
            }
        }
    }

    fn handle_event(&mut self, event: &mio::event::Event) -> bool {
        let Some(socket_id) = socket_id_from_token(event.token()) else {
            return true;
        };
        let Some(mut entry) = self.take_socket(socket_id) else {
            return true;
        };
        let mut accepted = Vec::new();
        let mut results = Vec::new();
        let keep = match &mut entry {
            SocketEntry::Listener {
                socket, accepts, ..
            } => {
                if event.is_readable() {
                    while let Some(id) = accepts.front().copied() {
                        match socket.accept() {
                            Ok((stream, _address)) => {
                                accepts.pop_front();
                                accepted.push((id, stream));
                            }
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                            Err(error) => {
                                accepts.pop_front();
                                results.push((id, IoResult::Failed(IoFailure::Accept(error))));
                            }
                        }
                    }
                }
                true
            }
            SocketEntry::Stream {
                socket,
                connect,
                reads,
                writes,
                ..
            } => {
                let mut keep = true;
                if event.is_writable() {
                    if let Some(request) = connect.as_ref() {
                        match socket.take_error() {
                            Ok(None) => match socket.peer_addr() {
                                Ok(_) => {
                                    let Some(request) = connect.take() else {
                                        solveig_base::ice!("connected socket lost its request")
                                    };
                                    results.push((request.id, IoResult::Socket(socket_id)));
                                }
                                Err(error)
                                    if matches!(
                                        error.kind(),
                                        io::ErrorKind::NotConnected | io::ErrorKind::WouldBlock
                                    ) => {}
                                Err(error) => {
                                    let Some(request) = connect.take() else {
                                        solveig_base::ice!("failed socket lost its request")
                                    };
                                    results.push((
                                        request.id,
                                        IoResult::Failed(IoFailure::Connect {
                                            host: request.host,
                                            port: request.port,
                                            source: error,
                                        }),
                                    ));
                                    keep = false;
                                }
                            },
                            Ok(Some(error)) | Err(error) => {
                                let request_id = request.id;
                                let Some(request) = connect.take() else {
                                    solveig_base::ice!("failed socket lost its request")
                                };
                                results.push((
                                    request_id,
                                    IoResult::Failed(IoFailure::Connect {
                                        host: request.host,
                                        port: request.port,
                                        source: error,
                                    }),
                                ));
                                keep = false;
                            }
                        }
                    }
                    if keep {
                        drive_writes(socket, writes, &mut results);
                    }
                }
                if keep && event.is_readable() {
                    drive_reads(socket, reads, &mut results);
                }
                keep
            }
        };

        if keep {
            self.put_socket(socket_id, entry);
            self.refresh(socket_id);
        } else {
            self.fail_entry(entry, "socket connection failed");
        }
        for (id, stream) in accepted {
            let accepted_id = self.next_socket_id();
            self.insert_socket(
                accepted_id,
                SocketEntry::Stream {
                    socket: stream,
                    connect: None,
                    reads: VecDeque::new(),
                    writes: VecDeque::new(),
                    registered: false,
                },
            );
            results.push((id, IoResult::Socket(accepted_id)));
        }
        for (id, result) in results {
            if !self.complete(id, result) {
                return false;
            }
        }
        true
    }

    fn refresh(&mut self, socket_id: SocketId) {
        let registry = self.poll.registry();
        let Some(entry) = self
            .sockets
            .get_mut(socket_id.0 as usize)
            .and_then(Option::as_mut)
        else {
            return;
        };
        if let Err(error) = entry.refresh(registry, socket_token(socket_id)) {
            let Some(entry) = self.take_socket(socket_id) else {
                solveig_base::ice!("socket vanished after registration failure")
            };
            self.fail_entry(entry, &format!("socket registration failed: {error}"));
        }
    }

    fn fail_entry(&self, entry: SocketEntry, message: &str) {
        for (id, failure) in entry.pending_failures(message) {
            self.complete(id, IoResult::Failed(failure));
        }
    }

    fn fail_all(&mut self, message: &str) {
        let entries = std::mem::take(&mut self.sockets);
        for entry in entries.into_iter().flatten() {
            self.fail_entry(entry, message);
        }
    }

    fn complete(&self, id: RequestId, result: IoResult) -> bool {
        self.completions.send(IoCompletion { id, result }).is_ok()
    }

    fn next_socket_id(&self) -> SocketId {
        let raw = u32::try_from(self.sockets.len())
            .unwrap_or_else(|_| solveig_base::ice!("socket table exhausted"));
        SocketId(raw)
    }

    fn insert_socket(&mut self, id: SocketId, entry: SocketEntry) {
        if id.0 as usize != self.sockets.len() {
            solveig_base::ice!("socket ids must be allocated monotonically")
        }
        self.sockets.push(Some(entry));
    }

    fn socket_mut(&mut self, id: SocketId) -> Option<&mut SocketEntry> {
        self.sockets.get_mut(id.0 as usize)?.as_mut()
    }

    fn take_socket(&mut self, id: SocketId) -> Option<SocketEntry> {
        self.sockets.get_mut(id.0 as usize)?.take()
    }

    fn put_socket(&mut self, id: SocketId, entry: SocketEntry) {
        let Some(slot) = self.sockets.get_mut(id.0 as usize) else {
            solveig_base::ice!("returned socket id was outside the side table")
        };
        if slot.replace(entry).is_some() {
            solveig_base::ice!("returned socket replaced a live side-table entry")
        }
    }
}
