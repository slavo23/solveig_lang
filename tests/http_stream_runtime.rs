//! Loopback HTTP streaming and scope cleanup, synchronized by requests and EOF.

use super::*;

#[test]
fn scoped_responses_pull_binary_chunks_and_close_on_every_exit() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("fixture binds");
    let base = format!("http://{}", listener.local_addr().expect("fixture address"));
    let server = thread::spawn(move || serve_scoped(&listener));
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http_stream_client");

    let output = run_project_capture_with_stdin_and_args(&root, "main", "", &[base])
        .expect("scoped client runs");

    assert_eq!(
        output,
        "gated:200:[Just 0, Just 255, Just 128, Just 97, Just 98, Just 99]:True\nempty:0\nexact:6\noverflow:limit\ntruncated:transport\nredirect:200\nno-redirect:302\nearly:closed\nerror:closed\ndefect:closed\ncancel:closed\nblocked:timeout\n"
    );
    server
        .join()
        .expect("fixture finishes after every connection closes");
}

fn serve_scoped(listener: &TcpListener) {
    for _ in 0..13 {
        let (mut stream, _) = listener.accept().expect("fixture accepts");
        let path = request_path(&mut stream);
        match path.as_str() {
            "/gated" => {
                write_response(&mut stream, b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
                acknowledge(listener, "/headers-seen");
                write_response(&mut stream, b"2\r\n\0\xff\r\n");
                acknowledge(listener, "/chunk-seen");
                write_response(&mut stream, b"4\r\n\x80abc\r\n0\r\n\r\n");
            }
            "/empty" | "/target" => write_response(&mut stream, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
            "/exact" => write_response(&mut stream, b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabcdef"),
            "/overflow" => write_response(&mut stream, b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nabcdefg"),
            "/truncated" => write_response(&mut stream, b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nabc"),
            "/redirect" => write_response(&mut stream, b"HTTP/1.1 302 Found\r\nLocation: /target\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
            "/early" | "/failure" | "/defect" | "/cancel" | "/blocked" => {
                // No body is sent. A collector, eager read, or draining close
                // deadlocks here and hits the watchdog rather than passing.
                write_response(&mut stream, b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n");
                let mut probe = [0; 1];
                assert_eq!(stream.read(&mut probe).unwrap_or_else(|error| panic!("scope {path} did not close its unread body: {error}")), 0);
            }
            other => panic!("unexpected scoped path {other}"),
        }
    }
}

fn acknowledge(listener: &TcpListener, expected: &str) {
    let (mut stream, _) = listener.accept().expect("callback issues acknowledgement");
    assert_eq!(request_path(&mut stream), expected);
    write_response(
        &mut stream,
        b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n",
    );
}
