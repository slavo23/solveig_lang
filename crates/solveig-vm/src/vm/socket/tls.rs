//! Rustls state driven by the existing nonblocking socket readiness loop.

use super::state::{ReadRequest, WriteRequest};
use crate::vm::RequestId;
use crate::vm::io_worker::{IoResult, TlsFailure};
use mio::Interest;
use mio::net::TcpStream;
use rustls::pki_types::{CertificateDer, ServerName, pem::PemObject};
use rustls::{ClientConfig, ClientConnection, RootCertStore};
use rustls_platform_verifier::ConfigVerifierExt;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::Arc;

const TLS_BUFFER_BYTES: usize = 65_536;

pub(super) struct TlsState {
    connection: ClientConnection,
    handshake: Option<RequestId>,
}

pub(super) struct TlsDriveFailure {
    pub(super) request: Option<RequestId>,
    pub(super) failure: TlsFailure,
}

impl TlsState {
    pub(super) fn new(
        request: RequestId,
        server_name: String,
        trust_pem: Option<Vec<u8>>,
    ) -> Result<Self, TlsFailure> {
        let server_name = ServerName::try_from(server_name).map_err(|error| {
            TlsFailure::Configuration(format!("invalid TLS server name: {error}"))
        })?;
        let config = match trust_pem {
            Some(pem) => custom_trust_config(&pem)?,
            None => ClientConfig::with_platform_verifier().map_err(|error| {
                TlsFailure::Configuration(format!(
                    "failed to initialize platform certificate verification: {error}"
                ))
            })?,
        };
        let mut connection =
            ClientConnection::new(Arc::new(config), server_name).map_err(|error| {
                TlsFailure::Configuration(format!("failed to initialize TLS client: {error}"))
            })?;
        connection.set_buffer_limit(Some(TLS_BUFFER_BYTES));
        Ok(Self {
            connection,
            handshake: Some(request),
        })
    }

    pub(super) fn interest(&self, has_reads: bool, has_writes: bool) -> Option<Interest> {
        let handshaking = self.handshake.is_some();
        let readable = self.connection.wants_read() && (handshaking || has_reads);
        let writable = self.connection.wants_write() || (!handshaking && has_writes);
        match (readable, writable) {
            (true, true) => Some(Interest::READABLE | Interest::WRITABLE),
            (true, false) => Some(Interest::READABLE),
            (false, true) => Some(Interest::WRITABLE),
            (false, false) => None,
        }
    }

    pub(super) fn handshake_request(&self) -> Option<RequestId> {
        self.handshake
    }

    pub(super) fn cancel(&mut self, id: RequestId) -> bool {
        if self.handshake == Some(id) {
            self.handshake = None;
            true
        } else {
            false
        }
    }

    pub(super) fn drive_handshake(
        &mut self,
        socket: &mut TcpStream,
    ) -> Result<Option<RequestId>, TlsFailure> {
        if self.handshake.is_none() {
            return Ok(None);
        }
        match self.connection.complete_io(socket) {
            Ok(_) if !self.connection.is_handshaking() && !self.connection.wants_write() => {
                Ok(self.handshake.take())
            }
            Ok(_) => Ok(None),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof
                ) =>
            {
                Err(TlsFailure::Protocol {
                    operation: "startTlsClient",
                    message: error.to_string(),
                })
            }
            Err(source) => Err(TlsFailure::Transport {
                operation: "startTlsClient",
                source,
            }),
        }
    }

    pub(super) fn drive_reads(
        &mut self,
        socket: &mut TcpStream,
        reads: &mut VecDeque<ReadRequest>,
        results: &mut Vec<(RequestId, IoResult)>,
    ) -> Result<(), TlsDriveFailure> {
        while let Some(request) = reads.front() {
            let mut bytes = vec![0; request.maximum];
            match self.connection.reader().read(&mut bytes) {
                Ok(read) => {
                    let Some(request) = reads.pop_front() else {
                        solveig_base::ice!("TLS read request vanished after successful receive")
                    };
                    bytes.truncate(read);
                    results.push((request.id, IoResult::Bytes(bytes)));
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    match self.connection.read_tls(socket) {
                        Ok(_) => {
                            if let Err(error) = self.connection.process_new_packets() {
                                return Err(TlsDriveFailure {
                                    request: Some(request.id),
                                    failure: TlsFailure::Protocol {
                                        operation: "recvBytes",
                                        message: error.to_string(),
                                    },
                                });
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                        Err(error) => {
                            return Err(TlsDriveFailure {
                                request: Some(request.id),
                                failure: TlsFailure::Transport {
                                    operation: "recvBytes",
                                    source: error,
                                },
                            });
                        }
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                    return Err(TlsDriveFailure {
                        request: Some(request.id),
                        failure: TlsFailure::Protocol {
                            operation: "recvBytes",
                            message: error.to_string(),
                        },
                    });
                }
                Err(error) => {
                    return Err(TlsDriveFailure {
                        request: Some(request.id),
                        failure: TlsFailure::Transport {
                            operation: "recvBytes",
                            source: error,
                        },
                    });
                }
            }
        }
        Ok(())
    }

    pub(super) fn drive_writes(
        &mut self,
        socket: &mut TcpStream,
        writes: &mut VecDeque<WriteRequest>,
        results: &mut Vec<(RequestId, IoResult)>,
    ) -> Result<(), TlsDriveFailure> {
        while let Some(request) = writes.front_mut() {
            if request.written < request.bytes.len() {
                let Some(remaining) = request.bytes.get(request.written..) else {
                    solveig_base::ice!("TLS write offset exceeded its buffer")
                };
                match self.connection.writer().write(remaining) {
                    Ok(0) => {
                        return Err(TlsDriveFailure {
                            request: Some(request.id),
                            failure: TlsFailure::Transport {
                                operation: "sendBytes",
                                source: io::Error::new(
                                    io::ErrorKind::WriteZero,
                                    "TLS plaintext writer accepted zero bytes",
                                ),
                            },
                        });
                    }
                    Ok(written) => request.written += written,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => {
                        return Err(TlsDriveFailure {
                            request: Some(request.id),
                            failure: TlsFailure::Transport {
                                operation: "sendBytes",
                                source: error,
                            },
                        });
                    }
                }
            }
            if !self.flush(socket).map_err(|failure| TlsDriveFailure {
                request: Some(request.id),
                failure,
            })? {
                return Ok(());
            }
            if request.written == request.bytes.len() {
                let Some(request) = writes.pop_front() else {
                    solveig_base::ice!("completed TLS write request vanished")
                };
                results.push((request.id, IoResult::Unit));
            } else {
                continue;
            }
        }
        self.flush(socket).map_err(|failure| TlsDriveFailure {
            request: None,
            failure,
        })?;
        Ok(())
    }

    pub(super) fn send_close_notify(&mut self, socket: &mut TcpStream) {
        self.connection.send_close_notify();
        drop(self.flush(socket));
    }

    fn flush(&mut self, socket: &mut TcpStream) -> Result<bool, TlsFailure> {
        while self.connection.wants_write() {
            match self.connection.write_tls(socket) {
                Ok(0) => {
                    return Err(TlsFailure::Transport {
                        operation: "sendBytes",
                        source: io::Error::new(
                            io::ErrorKind::WriteZero,
                            "TLS transport wrote zero bytes",
                        ),
                    });
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(error) => {
                    return Err(TlsFailure::Transport {
                        operation: "sendBytes",
                        source: error,
                    });
                }
            }
        }
        Ok(true)
    }
}

fn custom_trust_config(pem: &[u8]) -> Result<ClientConfig, TlsFailure> {
    let mut roots = RootCertStore::empty();
    let mut count = 0usize;
    for certificate in CertificateDer::pem_slice_iter(pem) {
        let certificate = certificate.map_err(|error| {
            TlsFailure::Configuration(format!("invalid trusted certificate PEM: {error}"))
        })?;
        roots.add(certificate).map_err(|error| {
            TlsFailure::Configuration(format!("invalid trusted certificate: {error}"))
        })?;
        count += 1;
    }
    if count == 0 {
        return Err(TlsFailure::Configuration(
            "trusted certificate PEM contains no certificates".to_string(),
        ));
    }
    Ok(ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_server_names_before_socket_io() {
        let result = TlsState::new(RequestId(0), "".to_string(), Some(Vec::new()));

        assert!(matches!(result, Err(TlsFailure::Configuration(_))));
    }

    #[test]
    fn rejects_empty_custom_trust_before_socket_io() {
        let result = TlsState::new(RequestId(0), "example.com".to_string(), Some(Vec::new()));

        assert!(matches!(result, Err(TlsFailure::Configuration(_))));
    }
}
