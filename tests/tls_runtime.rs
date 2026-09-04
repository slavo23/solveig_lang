use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use solveig_driver::run_project_capture_with_stdin_and_args;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const CERTIFICATE_PEM: &[u8] = include_bytes!("fixtures/tls_client/cert.pem");
const PRIVATE_KEY_PEM: &[u8] = include_bytes!("fixtures/tls_client/key.pem");

#[test]
fn scoped_tls_verifies_names_trust_binary_io_cancellation_and_cleanup() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("TLS fixture should bind");
    let port = listener
        .local_addr()
        .expect("TLS fixture should have an address")
        .port();
    let config = fixture_server_config();
    let server = thread::spawn(move || serve_connections(&listener, config));
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tls_client");
    let certificate = root.join("cert.pem").to_string_lossy().into_owned();

    let output = run_project_capture_with_stdin_and_args(
        &root,
        "main",
        "",
        &[port.to_string(), certificate],
    )
    .expect("TLS fixture client should run");

    assert_eq!(
        output,
        "reply:4:Just 0:Just 255:Just 128\n\
         name:protocol\n\
         trust:configuration\n\
         cancel:4:Just 0:Just 255:Just 128\n\
         truncated:protocol\n\
         chunk:chunk\n"
    );
    server.join().expect("TLS fixture server should finish");
}

fn fixture_server_config() -> Arc<ServerConfig> {
    let certificate = CertificateDer::from_pem_slice(CERTIFICATE_PEM)
        .expect("TLS fixture certificate should parse");
    let private_key =
        PrivateKeyDer::from_pem_slice(PRIVATE_KEY_PEM).expect("TLS fixture key should parse");
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate], private_key)
        .expect("TLS fixture certificate and key should agree");
    Arc::new(config)
}

fn serve_connections(listener: &TcpListener, config: Arc<ServerConfig>) {
    serve_exchange(listener, config.clone());
    serve_rejected_handshake(listener, config.clone());
    serve_plain_close(listener);
    serve_exchange(listener, config.clone());
    serve_truncated(listener, config.clone());
    serve_clean_close(listener, config);
}

fn serve_exchange(listener: &TcpListener, config: Arc<ServerConfig>) {
    let stream = accept_with_timeout(listener);
    let connection =
        ServerConnection::new(config).expect("TLS fixture connection should initialize");
    let mut tls = StreamOwned::new(connection, stream);
    let mut request = [0_u8; 4];
    tls.read_exact(&mut request)
        .expect("TLS fixture should read application bytes");
    assert_eq!(request, *b"ping");
    tls.write_all(&[0, 255])
        .expect("TLS fixture should write first binary part");
    tls.flush()
        .expect("TLS fixture should flush first binary part");
    tls.write_all(&[1, 128])
        .expect("TLS fixture should write second binary part");
    tls.conn.send_close_notify();
    tls.flush().expect("TLS fixture should flush close_notify");
}

fn serve_rejected_handshake(listener: &TcpListener, config: Arc<ServerConfig>) {
    let stream = accept_with_timeout(listener);
    let connection =
        ServerConnection::new(config).expect("TLS fixture connection should initialize");
    let mut tls = StreamOwned::new(connection, stream);
    let mut application_byte = [0_u8; 1];
    let result = tls.read(&mut application_byte);
    assert!(
        result.is_err(),
        "rejected TLS handshake should return an alert"
    );
}

fn serve_clean_close(listener: &TcpListener, config: Arc<ServerConfig>) {
    let stream = accept_with_timeout(listener);
    let connection =
        ServerConnection::new(config).expect("TLS fixture connection should initialize");
    let mut tls = StreamOwned::new(connection, stream);
    let mut application_byte = [0_u8; 1];
    assert_eq!(
        tls.read(&mut application_byte)
            .expect("TLS fixture should receive a clean close"),
        0
    );
}

fn serve_truncated(listener: &TcpListener, config: Arc<ServerConfig>) {
    let stream = accept_with_timeout(listener);
    let connection =
        ServerConnection::new(config).expect("TLS fixture connection should initialize");
    let mut tls = StreamOwned::new(connection, stream);
    let mut request = [0_u8; 4];
    tls.read_exact(&mut request)
        .expect("truncation fixture should read application bytes");
    assert_eq!(request, *b"ping");
    tls.write_all(&[7])
        .expect("truncation fixture should write application bytes");
    tls.flush()
        .expect("truncation fixture should flush application bytes");
}

fn serve_plain_close(listener: &TcpListener) {
    let mut stream = accept_with_timeout(listener);
    let mut byte = [0_u8; 1];
    assert_eq!(
        stream
            .read(&mut byte)
            .expect("invalid TLS configuration should close its TCP socket"),
        0
    );
}

fn accept_with_timeout(listener: &TcpListener) -> TcpStream {
    let (stream, _) = listener.accept().expect("TLS fixture should accept");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("TLS fixture read timeout should configure");
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .expect("TLS fixture write timeout should configure");
    stream
}
