use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::thread;

use solveig_driver::run_project_capture_with_stdin_and_args;

#[test]
fn scoped_uploads_are_demand_driven_and_transfer_the_response_resource() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("upload fixture should bind");
    let port = listener
        .local_addr()
        .expect("upload fixture should have an address")
        .port();
    let cross_listener =
        TcpListener::bind(("127.0.0.1", 0)).expect("cross-origin fixture should bind");
    let cross_port = cross_listener
        .local_addr()
        .expect("cross-origin fixture should have an address")
        .port();
    let cross_location = format!("http://127.0.0.1:{cross_port}/sensitive-target");
    let server = thread::spawn(move || serve_uploads(&listener, &cross_location));
    let cross_server = thread::spawn(move || serve_cross_origin(&cross_listener));
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http_upload_client");
    let base = format!("http://127.0.0.1:{port}");
    let payload = root.join("payload.txt").display().to_string();

    let output = run_project_capture_with_stdin_and_args(&root, "main", "", &[base, payload])
        .expect("upload fixture client should run");

    assert_eq!(
        output,
        "query:200:ok\n\
         text:200:ok\n\
         json:200:ok\n\
         form:200:ok\n\
         builder:200:ok\n\
         file:200:ok\n\
         redirect:200:ok\n\
         no-replay:blocked\n\
         cross-origin:200:ok\n\
         managed:Content-Length\n\
         strict:200:5\n\
         scoped:200:5\n\
         source-failure:planned\n\
         cancel-upload:closed\n\
         stream-replay:blocked\n"
    );
    server.join().expect("upload fixture server should finish");
    cross_server
        .join()
        .expect("cross-origin fixture server should finish");
}

fn serve_uploads(listener: &TcpListener, cross_location: &str) {
    for expected_path in [
        "/query?fixed=yes&q=snow+%E2%98%83&tag=a+b&tag=x%2Fy",
        "/text",
        "/json",
        "/form",
        "/builder",
        "/file",
        "/redirect-repeat",
        "/redirect-target",
        "/redirect-no-replay",
        "/cross-origin",
        "/strict",
        "/scoped",
        "/source-failure",
        "/cancel-upload",
        "/stream-redirect",
    ] {
        let (stream, _) = listener.accept().expect("upload fixture should accept");
        if matches!(expected_path, "/source-failure" | "/cancel-upload") {
            read_truncated_upload(stream, expected_path);
            continue;
        }
        let (mut stream, method, path, headers, body) = read_request(stream);
        assert_eq!(path, expected_path);
        let response = match path.as_str() {
            path if path.starts_with("/query?") => {
                assert_eq!(method, "GET");
                assert!(body.is_empty());
                ok_response("ok")
            }
            "/text" => {
                assert_request(
                    &method,
                    &headers,
                    &body,
                    "POST",
                    "text/plain; charset=utf-8",
                    "hello ✓",
                );
                ok_response("ok")
            }
            "/json" => {
                assert_request(
                    &method,
                    &headers,
                    &body,
                    "POST",
                    "application/vnd.solveig+json",
                    "{\"name\":\"Ada\"}",
                );
                ok_response("ok")
            }
            "/form" => {
                assert_request(
                    &method,
                    &headers,
                    &body,
                    "POST",
                    "application/x-www-form-urlencoded",
                    "name=Ada+Lovelace&tag=a%2Fb&tag=%E2%9C%93",
                );
                ok_response("ok")
            }
            "/builder" => {
                assert_eq!(method, "PUT");
                assert_eq!(body, b"builder");
                ok_response("ok")
            }
            "/file" => {
                assert_eq!(method, "PUT");
                assert_eq!(body, b"file payload\n");
                ok_response("ok")
            }
            "/redirect-repeat" | "/redirect-no-replay" | "/stream-redirect" => {
                redirect_response("/redirect-target")
            }
            "/redirect-target" => {
                assert_request(
                    &method,
                    &headers,
                    &body,
                    "POST",
                    "text/plain; charset=utf-8",
                    "repeat me",
                );
                ok_response("ok")
            }
            "/cross-origin" => redirect_response(cross_location),
            "/strict" | "/scoped" => {
                assert_eq!(body, b"abcde");
                ok_response(&body.len().to_string())
            }
            _ => panic!("unexpected upload fixture path {path}"),
        };
        stream
            .write_all(response.as_bytes())
            .expect("upload response should write");
    }
}

fn serve_cross_origin(listener: &TcpListener) {
    let (stream, _) = listener
        .accept()
        .expect("cross-origin fixture should accept");
    let (mut stream, method, path, headers, body) = read_request(stream);
    assert_eq!(method, "GET");
    assert_eq!(path, "/sensitive-target");
    assert!(body.is_empty());
    for sensitive in ["authorization", "cookie", "proxy-authorization"] {
        assert!(
            !headers.contains_key(sensitive),
            "cross-origin redirect retained {sensitive}"
        );
    }
    assert_eq!(headers.get("x-trace").map(String::as_str), Some("kept"));
    stream
        .write_all(ok_response("ok").as_bytes())
        .expect("cross-origin response should write");
}

fn read_truncated_upload(stream: TcpStream, expected_path: &str) {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .expect("truncated request line should read");
    assert_eq!(request_line.split_whitespace().nth(1), Some(expected_path));
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("truncated request header should read");
        if line == "\r\n" {
            break;
        }
    }
    let mut size_line = String::new();
    let size_count = reader
        .read_line(&mut size_line)
        .expect("partial upload chunk size should read");
    if expected_path == "/cancel-upload" {
        assert_eq!(
            size_count, 0,
            "cancelled upload should close before a chunk"
        );
        return;
    }
    let size = usize::from_str_radix(size_line.trim(), 16)
        .expect("partial upload chunk size should be hexadecimal");
    let mut bytes = vec![0_u8; size];
    reader
        .read_exact(&mut bytes)
        .expect("partial upload bytes should read");
    assert_eq!(bytes, b"partial");
    let mut ending = [0_u8; 2];
    reader
        .read_exact(&mut ending)
        .expect("partial upload chunk ending should read");
    assert_eq!(ending, *b"\r\n");

    let mut end = String::new();
    match reader.read_line(&mut end) {
        Ok(0) | Err(_) => {}
        Ok(_) => panic!("failed source unexpectedly completed its chunked upload"),
    }
}

fn assert_request(
    method: &str,
    headers: &BTreeMap<String, String>,
    body: &[u8],
    expected_method: &str,
    expected_content_type: &str,
    expected_body: &str,
) {
    assert_eq!(method, expected_method);
    assert_eq!(
        headers.get("content-type").map(String::as_str),
        Some(expected_content_type)
    );
    assert_eq!(body, expected_body.as_bytes());
}

fn ok_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn redirect_response(location: &str) -> String {
    format!(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
}

fn read_request(
    stream: TcpStream,
) -> (TcpStream, String, String, BTreeMap<String, String>, Vec<u8>) {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .expect("request line should read");
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .expect("request method should exist")
        .to_string();
    let path = request_parts
        .next()
        .expect("request path should exist")
        .to_string();
    let mut headers = BTreeMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("header should read");
        if line == "\r\n" {
            break;
        }
        let (name, value) = line
            .split_once(':')
            .expect("fixture header should have a colon");
        headers.insert(name.to_ascii_lowercase(), value.trim().to_string());
    }
    let mut body = Vec::new();
    if headers
        .get("transfer-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
    {
        loop {
            let mut size_line = String::new();
            reader
                .read_line(&mut size_line)
                .expect("chunk size should read");
            let size = usize::from_str_radix(size_line.trim(), 16)
                .expect("chunk size should be hexadecimal");
            if size == 0 {
                let mut trailer_end = String::new();
                reader
                    .read_line(&mut trailer_end)
                    .expect("chunk trailer end should read");
                break;
            }
            let start = body.len();
            body.resize(start + size, 0);
            let Some(chunk) = body.get_mut(start..) else {
                panic!("resized request body lost its appended chunk")
            };
            reader.read_exact(chunk).expect("chunk bytes should read");
            let mut ending = [0_u8; 2];
            reader
                .read_exact(&mut ending)
                .expect("chunk ending should read");
            assert_eq!(ending, *b"\r\n");
        }
    } else if let Some(length) = headers.get("content-length") {
        let length = length
            .parse::<usize>()
            .expect("content length should parse");
        body.resize(length, 0);
        reader
            .read_exact(&mut body)
            .expect("request body should read");
    }
    (reader.into_inner(), method, path, headers, body)
}
