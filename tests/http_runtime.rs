#![expect(
    clippy::panic,
    reason = "integration tests use panic messages for deterministic fixture-server failures"
)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::thread;
use std::time::Duration;

use solveig_driver::run_project_capture_with_stdin_and_args;

#[test]
fn bounded_http_handles_framing_redirects_status_limits_and_cancellation() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("HTTP fixture should bind");
    let port = listener
        .local_addr()
        .expect("HTTP fixture should have an address")
        .port();
    let server = thread::spawn(move || serve(listener, 11));
    let tls_listener = TcpListener::bind(("127.0.0.1", 0)).expect("TLS fixture should bind");
    let tls_port = tls_listener
        .local_addr()
        .expect("TLS fixture should have an address")
        .port();
    let tls_server = thread::spawn(move || reject_tls(tls_listener));
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http_client");
    let base = format!("http://127.0.0.1:{port}");
    let tls_base = format!("https://127.0.0.1:{tls_port}");

    let output = run_project_capture_with_stdin_and_args(&root, "main", "", &[base, tls_base])
        .expect("HTTP fixture client should run");

    assert_eq!(
        output,
        "fetch:200:Just \"fixture-v1\":hello world\n\
         chunked:200:Just \"fixture-v1\":hello world\n\
         redirect:200:Just \"redirected\":arrived\n\
         status:404:Nothing:missing\n\
         body-limit:body-too-large:4\n\
         header-limit:headers-too-large:80\n\
         malformed:transport\n\
         redirect-limit:redirect\n\
         host-timeout:timeout\n\
         tls:tls\n\
         cancelled:prompt\n"
    );
    server.join().expect("HTTP fixture server should finish");
    tls_server.join().expect("TLS fixture server should finish");
}

fn reject_tls(listener: TcpListener) {
    let (mut stream, _) = listener.accept().expect("TLS fixture should accept");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("TLS fixture read timeout should configure");
    let mut hello = [0_u8; 1024];
    let _received = stream
        .read(&mut hello)
        .expect("TLS fixture should receive a client hello");
    write_response(
        &mut stream,
        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    drop(listener);
}

fn serve(listener: TcpListener, request_count: usize) {
    for _ in 0..request_count {
        let (mut stream, _) = listener.accept().expect("HTTP fixture should accept");
        let path = request_path(&mut stream);
        match path.as_str() {
            "/chunked" => write_response(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nETag: fixture-v1\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n",
            ),
            "/redirect" => write_response(
                &mut stream,
                b"HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            ),
            "/redirect-loop" => write_response(
                &mut stream,
                b"HTTP/1.1 302 Found\r\nLocation: /redirect-loop\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            ),
            "/final" => write_response(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nETag: redirected\r\nContent-Length: 7\r\nConnection: close\r\n\r\narrived",
            ),
            "/missing" => write_response(
                &mut stream,
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 7\r\nConnection: close\r\n\r\nmissing",
            ),
            "/large" => write_response(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n12345",
            ),
            "/large-header" => write_response(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nX-Fixture-Padding: abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            ),
            "/malformed" => write_response(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\nZ\r\nbad\r\n",
            ),
            "/slow" | "/slow-timeout" => {
                thread::sleep(Duration::from_millis(100));
                write_response(
                    &mut stream,
                    b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nlate",
                );
            }
            other => panic!("unexpected HTTP fixture path {other}"),
        }
    }
    drop(listener);
}

fn request_path(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("HTTP fixture read timeout should configure");
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 512];
    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).expect("HTTP request should read");
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(
            buffer
                .get(..count)
                .expect("read byte count should fit its destination buffer"),
        );
    }
    let request = String::from_utf8(bytes).expect("HTTP fixture request should be ASCII");
    request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("<missing-path>")
        .to_string()
}

fn write_response(stream: &mut TcpStream, response: &[u8]) {
    stream
        .write_all(response)
        .expect("HTTP fixture response should write");
}
