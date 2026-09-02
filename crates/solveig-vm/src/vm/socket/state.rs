//! Per-resource TCP state and readiness-driven IO operations.

use crate::vm::io_worker::{IoFailure, IoResult};
use crate::vm::{RequestId, SocketId};
use mio::net::{TcpListener, TcpStream, UdpSocket};
use mio::{Interest, Registry, Token};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub(super) enum SocketEntry {
    Listener {
        socket: TcpListener,
        accepts: VecDeque<RequestId>,
        registered: bool,
    },
    Stream {
        socket: TcpStream,
        connect: Option<ConnectRequest>,
        reads: VecDeque<ReadRequest>,
        writes: VecDeque<WriteRequest>,
        registered: bool,
    },
    Datagram {
        socket: UdpSocket,
        reads: VecDeque<DatagramReadRequest>,
        writes: VecDeque<DatagramWriteRequest>,
        registered: bool,
    },
}

pub(super) struct ReadRequest {
    pub(super) id: RequestId,
    pub(super) maximum: usize,
}

pub(super) struct ConnectRequest {
    pub(super) id: RequestId,
    pub(super) host: String,
    pub(super) port: i64,
}

pub(super) struct WriteRequest {
    pub(super) id: RequestId,
    pub(super) bytes: Vec<u8>,
    pub(super) written: usize,
}

pub(super) struct DatagramReadRequest {
    pub(super) id: RequestId,
    pub(super) maximum: usize,
}

pub(super) struct DatagramWriteRequest {
    pub(super) id: RequestId,
    pub(super) address: SocketAddr,
    pub(super) bytes: Vec<u8>,
}

impl SocketEntry {
    pub(super) fn is_registered(&self) -> bool {
        match self {
            Self::Listener { registered, .. }
            | Self::Stream { registered, .. }
            | Self::Datagram { registered, .. } => *registered,
        }
    }

    pub(super) fn deregister(&mut self, registry: &Registry) -> io::Result<()> {
        match self {
            Self::Listener {
                socket, registered, ..
            } => {
                let result = registry.deregister(socket);
                *registered = false;
                result
            }
            Self::Stream {
                socket, registered, ..
            } => {
                let result = registry.deregister(socket);
                *registered = false;
                result
            }
            Self::Datagram {
                socket, registered, ..
            } => {
                let result = registry.deregister(socket);
                *registered = false;
                result
            }
        }
    }

    pub(super) fn refresh(&mut self, registry: &Registry, token: Token) -> io::Result<()> {
        let interest = self.interest();
        match (self, interest) {
            (
                Self::Listener {
                    socket, registered, ..
                },
                Some(interest),
            ) => {
                if *registered {
                    registry.reregister(socket, token, interest)
                } else {
                    registry.register(socket, token, interest)?;
                    *registered = true;
                    Ok(())
                }
            }
            (
                Self::Stream {
                    socket, registered, ..
                },
                Some(interest),
            ) => {
                if *registered {
                    registry.reregister(socket, token, interest)
                } else {
                    registry.register(socket, token, interest)?;
                    *registered = true;
                    Ok(())
                }
            }
            (
                Self::Datagram {
                    socket, registered, ..
                },
                Some(interest),
            ) => {
                if *registered {
                    registry.reregister(socket, token, interest)
                } else {
                    registry.register(socket, token, interest)?;
                    *registered = true;
                    Ok(())
                }
            }
            (entry, None) if entry.is_registered() => entry.deregister(registry),
            (Self::Listener { .. } | Self::Stream { .. } | Self::Datagram { .. }, None) => Ok(()),
        }
    }

    fn interest(&self) -> Option<Interest> {
        match self {
            Self::Listener { accepts, .. } => (!accepts.is_empty()).then_some(Interest::READABLE),
            Self::Stream {
                connect,
                reads,
                writes,
                ..
            } => match (!reads.is_empty(), connect.is_some() || !writes.is_empty()) {
                (true, true) => Some(Interest::READABLE | Interest::WRITABLE),
                (true, false) => Some(Interest::READABLE),
                (false, true) => Some(Interest::WRITABLE),
                (false, false) => None,
            },
            Self::Datagram { reads, writes, .. } => match (!reads.is_empty(), !writes.is_empty()) {
                (true, true) => Some(Interest::READABLE | Interest::WRITABLE),
                (true, false) => Some(Interest::READABLE),
                (false, true) => Some(Interest::WRITABLE),
                (false, false) => None,
            },
        }
    }

    pub(super) fn cancel(&mut self, id: RequestId) -> CancelEffect {
        match self {
            Self::Listener { accepts, .. } => {
                if remove_request(accepts, id) {
                    CancelEffect::Keep
                } else {
                    CancelEffect::Absent
                }
            }
            Self::Stream {
                connect,
                reads,
                writes,
                ..
            } => {
                if connect.as_ref().is_some_and(|request| request.id == id) {
                    *connect = None;
                    CancelEffect::Close
                } else if let Some(index) = reads.iter().position(|request| request.id == id) {
                    reads.remove(index);
                    CancelEffect::Keep
                } else if let Some(index) = writes.iter().position(|request| request.id == id) {
                    writes.remove(index);
                    CancelEffect::Keep
                } else {
                    CancelEffect::Absent
                }
            }
            Self::Datagram { reads, writes, .. } => {
                if let Some(index) = reads.iter().position(|request| request.id == id) {
                    reads.remove(index);
                    CancelEffect::Keep
                } else if let Some(index) = writes.iter().position(|request| request.id == id) {
                    writes.remove(index);
                    CancelEffect::Keep
                } else {
                    CancelEffect::Absent
                }
            }
        }
    }

    pub(super) fn pending_failures(self, message: &str) -> Vec<(RequestId, IoFailure)> {
        match self {
            Self::Listener { accepts, .. } => accepts
                .into_iter()
                .map(|id| (id, IoFailure::Accept(io::Error::other(message.to_string()))))
                .collect(),
            Self::Stream {
                connect,
                reads,
                writes,
                ..
            } => {
                let mut failures = Vec::new();
                if let Some(id) = connect {
                    failures.push((
                        id.id,
                        IoFailure::Connect {
                            host: id.host,
                            port: id.port,
                            source: io::Error::other(message.to_string()),
                        },
                    ));
                }
                failures.extend(reads.into_iter().map(|request| {
                    (
                        request.id,
                        IoFailure::Recv(io::Error::other(message.to_string())),
                    )
                }));
                failures.extend(writes.into_iter().map(|request| {
                    (
                        request.id,
                        IoFailure::Send(io::Error::other(message.to_string())),
                    )
                }));
                failures
            }
            Self::Datagram { reads, writes, .. } => reads
                .into_iter()
                .map(|request| {
                    (
                        request.id,
                        IoFailure::RecvDatagram(io::Error::other(message.to_string())),
                    )
                })
                .chain(writes.into_iter().map(|request| {
                    (
                        request.id,
                        IoFailure::SendDatagram(io::Error::other(message.to_string())),
                    )
                }))
                .collect(),
        }
    }
}

pub(super) enum CancelEffect {
    Absent,
    Keep,
    Close,
}

pub(super) fn drive_reads(
    socket: &mut TcpStream,
    reads: &mut VecDeque<ReadRequest>,
    results: &mut Vec<(RequestId, IoResult)>,
) {
    while let Some(request) = reads.front() {
        let mut bytes = vec![0; request.maximum];
        match socket.read(&mut bytes) {
            Ok(read) => {
                let Some(request) = reads.pop_front() else {
                    solveig_base::ice!("read request vanished after successful receive")
                };
                bytes.truncate(read);
                results.push((request.id, IoResult::Bytes(bytes)));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
            Err(error) => {
                let Some(request) = reads.pop_front() else {
                    solveig_base::ice!("read request vanished after failed receive")
                };
                results.push((request.id, IoResult::Failed(IoFailure::Recv(error))));
            }
        }
    }
}

pub(super) fn drive_writes(
    socket: &mut TcpStream,
    writes: &mut VecDeque<WriteRequest>,
    results: &mut Vec<(RequestId, IoResult)>,
) {
    while let Some(request) = writes.front_mut() {
        let Some(remaining) = request.bytes.get(request.written..) else {
            solveig_base::ice!("socket write offset exceeded its buffer")
        };
        match socket.write(remaining) {
            Ok(0) => {
                let Some(request) = writes.pop_front() else {
                    solveig_base::ice!("write request vanished after zero-length write")
                };
                results.push((
                    request.id,
                    IoResult::Failed(IoFailure::Send(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "socket write returned zero before the buffer was sent",
                    ))),
                ));
            }
            Ok(written) => {
                request.written += written;
                if request.written == request.bytes.len() {
                    let Some(request) = writes.pop_front() else {
                        solveig_base::ice!("completed write request vanished")
                    };
                    results.push((request.id, IoResult::Unit));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
            Err(error) => {
                let Some(request) = writes.pop_front() else {
                    solveig_base::ice!("write request vanished after failed send")
                };
                results.push((request.id, IoResult::Failed(IoFailure::Send(error))));
            }
        }
    }
}

/// Drives whole UDP messages according to RFC 768 datagram boundaries.
pub(super) fn drive_datagram_reads(
    socket: &mut UdpSocket,
    reads: &mut VecDeque<DatagramReadRequest>,
    results: &mut Vec<(RequestId, IoResult)>,
) {
    while let Some(request) = reads.front() {
        let Some(buffer_size) = request.maximum.checked_add(1) else {
            solveig_base::ice!("validated datagram receive bound overflowed")
        };
        let mut bytes = vec![0; buffer_size];
        match socket.recv_from(&mut bytes) {
            Ok((read, address)) => {
                let Some(request) = reads.pop_front() else {
                    solveig_base::ice!("datagram read vanished after successful receive")
                };
                if read > request.maximum {
                    let maximum = i64::try_from(request.maximum)
                        .unwrap_or_else(|_| solveig_base::ice!("datagram bound exceeded Int"));
                    results.push((
                        request.id,
                        IoResult::Failed(IoFailure::DatagramTooLarge { maximum }),
                    ));
                    continue;
                }
                bytes.truncate(read);
                results.push((
                    request.id,
                    IoResult::Datagram {
                        bytes,
                        host: address.ip().to_string(),
                        port: address.port(),
                    },
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
            Err(error) => {
                let Some(request) = reads.pop_front() else {
                    solveig_base::ice!("datagram read vanished after failed receive")
                };
                results.push((request.id, IoResult::Failed(IoFailure::RecvDatagram(error))));
            }
        }
    }
}

/// Sends one UDP message per request without coalescing or splitting it.
pub(super) fn drive_datagram_writes(
    socket: &mut UdpSocket,
    writes: &mut VecDeque<DatagramWriteRequest>,
    results: &mut Vec<(RequestId, IoResult)>,
) {
    while let Some(request) = writes.front() {
        match socket.send_to(&request.bytes, request.address) {
            Ok(written) if written == request.bytes.len() => {
                let Some(request) = writes.pop_front() else {
                    solveig_base::ice!("datagram write vanished after successful send")
                };
                results.push((request.id, IoResult::Unit));
            }
            Ok(written) => {
                let Some(request) = writes.pop_front() else {
                    solveig_base::ice!("datagram write vanished after partial send")
                };
                results.push((
                    request.id,
                    IoResult::Failed(IoFailure::SendDatagram(io::Error::new(
                        io::ErrorKind::WriteZero,
                        format!(
                            "datagram send wrote {written} of {} bytes",
                            request.bytes.len()
                        ),
                    ))),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
            Err(error) => {
                let Some(request) = writes.pop_front() else {
                    solveig_base::ice!("datagram write vanished after failed send")
                };
                results.push((request.id, IoResult::Failed(IoFailure::SendDatagram(error))));
            }
        }
    }
}

fn remove_request(requests: &mut VecDeque<RequestId>, id: RequestId) -> bool {
    let Some(index) = requests.iter().position(|request| *request == id) else {
        return false;
    };
    requests.remove(index);
    true
}

pub(super) fn socket_address(host: &str, port: i64) -> io::Result<SocketAddr> {
    let port = valid_port(port)?;
    let ip = if host == "localhost" {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        host.parse::<IpAddr>().map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("host must be an IP address or localhost: {error}"),
            )
        })?
    };
    Ok(SocketAddr::new(ip, port))
}

pub(super) fn loopback_address(port: i64) -> io::Result<SocketAddr> {
    Ok(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        valid_port(port)?,
    ))
}

fn valid_port(port: i64) -> io::Result<u16> {
    u16::try_from(port).map_err(|_range| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "network port must be between 0 and 65535",
        )
    })
}

pub(super) fn socket_token(id: SocketId) -> Token {
    let token = (id.0 as usize)
        .checked_add(1)
        .unwrap_or_else(|| solveig_base::ice!("socket token overflow"));
    Token(token)
}

pub(super) fn socket_id_from_token(token: Token) -> Option<SocketId> {
    let raw = token.0.checked_sub(1)?;
    u32::try_from(raw).ok().map(SocketId)
}

pub(super) fn closed_socket_error() -> io::Error {
    io::Error::new(io::ErrorKind::NotConnected, "socket is closed")
}
