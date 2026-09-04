//! Bounded HTTP request preparation and blocking host execution.
//! VM values are decoded on the scheduler thread; workers receive only owned
//! Rust data; strict collection and scoped acquisition share one protocol path.

use super::*;
use crate::builtins::VmCtx;
use crate::heap::HeapObject;
use solveig_ast::ast::CtorRuntimeRole;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use ureq::RequestExt;
use ureq::tls::{RootCerts, TlsConfig};

pub(super) struct HttpRequest {
    pub(super) method: String,
    pub(super) url: String,
    pub(super) headers: Vec<(String, String)>,
    pub(super) query: Vec<(String, String)>,
    pub(super) body: HttpRequestBody,
    pub(super) maximum_headers: usize,
    pub(super) maximum_body: Option<usize>,
    pub(super) timeout: Duration,
    pub(super) maximum_redirects: u32,
    pub(super) replay_body: bool,
}

pub(super) enum HttpRequestBody {
    Empty,
    Bytes(Vec<u8>),
    Chunks(Vec<Vec<u8>>),
    File(PathBuf),
    Form(Vec<(String, String)>),
    StreamPending,
    Stream(Box<dyn Read + Send>),
}

pub(super) struct HttpResponse<B> {
    pub(super) status: i64,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: B,
}

pub(super) enum HttpFailure {
    Transport(String),
    Tls(String),
    Timeout(String),
    Redirect(String),
    Replay(String),
    RequestFile(String),
    HeadersTooLarge(usize),
    BodyTooLarge(usize),
}

impl Interpreter<'_> {
    pub(super) fn prepare_http_request(
        &mut self,
        request: Value,
        limits: Value,
    ) -> Result<HttpRequest, RuntimeError> {
        let builtin = "httpRequestRaw";
        let [method, url, headers, query, body] = self.expect_http_tuple::<5>(request)?;
        let [
            maximum_headers,
            maximum_body,
            timeout_milliseconds,
            maximum_redirects,
            replay_body,
        ] = self.expect_http_tuple::<5>(limits)?;
        let maximum_headers = expect_http_int(maximum_headers, "header limit")?;
        let timeout_milliseconds = expect_http_int(timeout_milliseconds, "timeout")?;
        let maximum_redirects = expect_http_int(maximum_redirects, "redirect limit")?;
        let maximum_headers = positive_usize(maximum_headers, "header limit")?;
        let maximum_body = self
            .expect_optional_nonnegative_int(maximum_body, builtin)?
            .map(|maximum| nonnegative_usize(maximum, "body limit"))
            .transpose()?;
        let timeout_milliseconds = positive_u64(timeout_milliseconds, "timeout")?;
        let maximum_redirects = nonnegative_u32(maximum_redirects, "redirect limit")?;
        Ok(HttpRequest {
            method: self.expect_string_value(method, builtin)?,
            url: self.expect_string_value(url, builtin)?,
            headers: self.expect_string_pair_list_value(headers, builtin)?,
            query: self.expect_string_pair_list_value(query, builtin)?,
            body: self.expect_http_body(body)?,
            maximum_headers,
            maximum_body,
            timeout: Duration::from_millis(timeout_milliseconds),
            maximum_redirects,
            replay_body: self.expect_bool_value(replay_body, builtin)?,
        })
    }

    fn expect_http_tuple<const N: usize>(
        &mut self,
        value: Value,
    ) -> Result<[Value; N], RuntimeError> {
        let builtin = "httpRequestRaw";
        let Ok(role_arity) = u8::try_from(N) else {
            solveig_base::ice!("HTTP tuple arity does not fit in the runtime role");
        };
        let Ok(field_arity) = u16::try_from(N) else {
            solveig_base::ice!("HTTP tuple arity does not fit in a constructor tag");
        };
        let tag = VmCtx::new(self).ctor_tag_with_arity(
            CtorRuntimeRole::PreludeTuple(role_arity),
            field_arity,
            builtin,
        )?;
        let Value::Obj(id) = value else {
            return Err(RuntimeError::builtin(format!(
                "httpRequestRaw expected a {N}-tuple payload"
            )));
        };
        let result = match self.heap.get(id) {
            HeapObject::Data2 {
                tag: actual_tag,
                field0,
                field1,
            } if N == 2 && *actual_tag == tag => vec![*field0, *field1],
            HeapObject::DataN {
                tag: actual_tag,
                fields,
            } if N > 2 && *actual_tag == tag => self.heap.payload_slice(id, *fields).to_vec(),
            _ => Vec::new(),
        };
        result.try_into().map_err(|_values| {
            RuntimeError::builtin(format!("httpRequestRaw expected a {N}-tuple payload"))
        })
    }

    fn expect_http_body(&mut self, value: Value) -> Result<HttpRequestBody, RuntimeError> {
        let builtin = "httpRequestRaw";
        let [tag, bytes, chunks, path, form] = self.expect_http_tuple::<5>(value)?;
        let tag = expect_http_int(tag, "body tag")?;
        match tag {
            0 => Ok(HttpRequestBody::Empty),
            1 => self
                .expect_optional_http_body(bytes)?
                .map(HttpRequestBody::Bytes)
                .ok_or_else(|| {
                    RuntimeError::builtin("httpRequestRaw byte body is missing".to_string())
                }),
            2 => self
                .expect_http_byte_list(chunks)
                .map(HttpRequestBody::Chunks),
            3 => self
                .expect_optional_http_path(path)?
                .map(|path| HttpRequestBody::File(self.io.resolve_path(&path)))
                .ok_or_else(|| {
                    RuntimeError::builtin("httpRequestRaw file body path is missing".to_string())
                }),
            4 => self
                .expect_string_pair_list_value(form, builtin)
                .map(HttpRequestBody::Form),
            5 => Ok(HttpRequestBody::StreamPending),
            _ => Err(RuntimeError::builtin(format!(
                "httpRequestRaw invalid body tag {tag}"
            ))),
        }
    }

    fn expect_http_byte_list(&mut self, mut current: Value) -> Result<Vec<Vec<u8>>, RuntimeError> {
        let builtin = "httpRequestRaw";
        let nil_tag =
            VmCtx::new(self).ctor_tag_with_arity(CtorRuntimeRole::PreludeListNil, 0, builtin)?;
        let cons_tag =
            VmCtx::new(self).ctor_tag_with_arity(CtorRuntimeRole::PreludeListCons, 2, builtin)?;
        let mut chunks = Vec::new();

        loop {
            let Value::Obj(id) = current else {
                return Err(RuntimeError::builtin(
                    "httpRequestRaw expected a [Bytes] body chunk list".to_string(),
                ));
            };
            match self.heap.get(id) {
                HeapObject::Data0 { tag } if *tag == nil_tag => return Ok(chunks),
                HeapObject::Data2 {
                    tag,
                    field0,
                    field1,
                } if *tag == cons_tag => {
                    chunks.push(self.expect_bytes_value(*field0, builtin)?);
                    current = *field1;
                }
                _ => {
                    return Err(RuntimeError::builtin(
                        "httpRequestRaw expected a [Bytes] body chunk list".to_string(),
                    ));
                }
            }
        }
    }

    pub(super) fn expect_optional_http_body(
        &mut self,
        value: Value,
    ) -> Result<Option<Vec<u8>>, RuntimeError> {
        self.expect_optional_bytes_value(value, "httpRequestRaw")
    }

    fn expect_optional_http_path(&mut self, value: Value) -> Result<Option<String>, RuntimeError> {
        let builtin = "httpRequestRaw";
        let nothing = VmCtx::new(self).ctor_tag_with_arity(
            CtorRuntimeRole::PreludeMaybeNothing,
            0,
            builtin,
        )?;
        let just =
            VmCtx::new(self).ctor_tag_with_arity(CtorRuntimeRole::PreludeMaybeJust, 1, builtin)?;
        let Value::Obj(id) = value else {
            return Err(RuntimeError::builtin(
                "httpRequestRaw expected Maybe String".to_string(),
            ));
        };
        match self.heap.get(id) {
            HeapObject::Data0 { tag } if *tag == nothing => Ok(None),
            HeapObject::Data1 { tag, field } if *tag == just => {
                self.expect_string_value(*field, builtin).map(Some)
            }
            _ => Err(RuntimeError::builtin(
                "httpRequestRaw expected Maybe String".to_string(),
            )),
        }
    }
}

pub(super) fn agent() -> ureq::Agent {
    let tls = TlsConfig::builder()
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .build()
        .new_agent()
}

pub(super) fn open_response(
    agent: &ureq::Agent,
    request: HttpRequest,
) -> Result<HttpResponse<ureq::Body>, HttpFailure> {
    let mut url = url::Url::parse(&request.url)
        .map_err(|error| HttpFailure::Transport(format!("invalid URL: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err(HttpFailure::Transport(
            "URL must be absolute HTTP or HTTPS".to_string(),
        ));
    }
    if !request.query.is_empty() {
        let mut query = url.query_pairs_mut();
        for (name, value) in &request.query {
            query.append_pair(name, value);
        }
    }

    let mut method = ureq::http::Method::from_bytes(request.method.as_bytes())
        .map_err(|error| HttpFailure::Transport(error.to_string()))?;
    let mut headers = request.headers;
    let mut body = PreparedBody::new(request.body)?;
    let started = Instant::now();
    let mut redirects = 0_u32;

    loop {
        let remaining = request
            .timeout
            .checked_sub(started.elapsed())
            .ok_or_else(|| HttpFailure::Timeout("whole-request deadline expired".to_string()))?;
        let response = body.send(
            agent,
            &method,
            &url,
            &headers,
            request.maximum_headers,
            remaining,
        )?;
        let status = response.status().as_u16();
        if !is_redirect(status) {
            return response_from_ureq(response);
        }

        let Some(location) = response.headers().get(ureq::http::header::LOCATION) else {
            return response_from_ureq(response);
        };
        if request.maximum_redirects == 0 {
            return response_from_ureq(response);
        }
        if redirects >= request.maximum_redirects {
            return Err(HttpFailure::Redirect(format!(
                "more than {} redirects",
                request.maximum_redirects
            )));
        }
        let location = location
            .to_str()
            .map_err(|error| HttpFailure::Redirect(format!("invalid Location header: {error}")))?;
        let target = url
            .join(location)
            .map_err(|error| HttpFailure::Redirect(format!("invalid redirect URL: {error}")))?;
        if !matches!(target.scheme(), "http" | "https") || target.host().is_none() {
            return Err(HttpFailure::Redirect(
                "redirect target must be absolute HTTP or HTTPS".to_string(),
            ));
        }

        let drops_body = should_drop_body(status, &method);
        if drops_body {
            method = ureq::http::Method::GET;
            body = PreparedBody::Empty;
            headers.retain(|(name, _)| {
                !name.eq_ignore_ascii_case("content-type")
                    && !name.eq_ignore_ascii_case("content-encoding")
            });
        } else if body.has_body() && (body.is_one_shot() || !request.replay_body) {
            return Err(HttpFailure::Replay(format!(
                "status {status} requires replaying the request body"
            )));
        }
        if url.origin() != target.origin() {
            headers.retain(|(name, _)| !is_sensitive_header(name));
        }
        drop(response);
        url = target;
        redirects += 1;
    }
}

enum PreparedBody {
    Empty,
    Bytes(Vec<u8>),
    Chunks(Vec<Vec<u8>>),
    File(File),
    OneShot(Box<dyn Read + Send>),
}

impl PreparedBody {
    fn new(body: HttpRequestBody) -> Result<Self, HttpFailure> {
        match body {
            HttpRequestBody::Empty => Ok(Self::Empty),
            HttpRequestBody::Bytes(bytes) => Ok(Self::Bytes(bytes)),
            HttpRequestBody::Chunks(chunks) => Ok(Self::Chunks(chunks)),
            HttpRequestBody::File(path) => File::open(&path)
                .map(Self::File)
                .map_err(|error| HttpFailure::RequestFile(format!("{}: {error}", path.display()))),
            HttpRequestBody::Form(pairs) => {
                let mut serializer = url::form_urlencoded::Serializer::new(String::new());
                serializer.extend_pairs(pairs);
                Ok(Self::Bytes(serializer.finish().into_bytes()))
            }
            HttpRequestBody::StreamPending => Err(HttpFailure::Transport(
                "stream upload was not attached to its host reader".to_string(),
            )),
            HttpRequestBody::Stream(reader) => Ok(Self::OneShot(reader)),
        }
    }

    fn has_body(&self) -> bool {
        !matches!(self, Self::Empty)
    }

    fn is_one_shot(&self) -> bool {
        matches!(self, Self::OneShot(_))
    }

    fn send(
        &mut self,
        agent: &ureq::Agent,
        method: &ureq::http::Method,
        url: &url::Url,
        headers: &[(String, String)],
        maximum_headers: usize,
        timeout: Duration,
    ) -> Result<ureq::http::Response<ureq::Body>, HttpFailure> {
        match self {
            Self::Empty => send_attempt(agent, method, url, headers, maximum_headers, timeout, ()),
            Self::Bytes(bytes) => send_attempt(
                agent,
                method,
                url,
                headers,
                maximum_headers,
                timeout,
                bytes.as_slice(),
            ),
            Self::Chunks(chunks) => {
                let mut reader = ChunkReader::new(chunks);
                send_attempt(
                    agent,
                    method,
                    url,
                    headers,
                    maximum_headers,
                    timeout,
                    ureq::SendBody::from_reader(&mut reader),
                )
            }
            Self::File(file) => {
                file.seek(SeekFrom::Start(0))
                    .map_err(|error| HttpFailure::RequestFile(error.to_string()))?;
                send_attempt(
                    agent,
                    method,
                    url,
                    headers,
                    maximum_headers,
                    timeout,
                    ureq::SendBody::from_reader(file),
                )
            }
            Self::OneShot(reader) => send_attempt(
                agent,
                method,
                url,
                headers,
                maximum_headers,
                timeout,
                ureq::SendBody::from_reader(reader.as_mut()),
            ),
        }
    }
}

struct ChunkReader<'a> {
    chunks: std::slice::Iter<'a, Vec<u8>>,
    current: &'a [u8],
}

impl<'a> ChunkReader<'a> {
    fn new(chunks: &'a [Vec<u8>]) -> Self {
        Self {
            chunks: chunks.iter(),
            current: &[],
        }
    }
}

impl Read for ChunkReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if !self.current.is_empty() {
                let count = self.current.len().min(output.len());
                let Some(target) = output.get_mut(..count) else {
                    solveig_base::ice!("HTTP chunk output bound was outside its buffer")
                };
                let Some((source, rest)) = self.current.split_at_checked(count) else {
                    solveig_base::ice!("HTTP chunk read bound was outside its source")
                };
                target.copy_from_slice(source);
                self.current = rest;
                return Ok(count);
            }
            let Some(next) = self.chunks.next() else {
                return Ok(0);
            };
            self.current = next;
        }
    }
}

fn send_attempt<B: ureq::AsSendBody>(
    agent: &ureq::Agent,
    method: &ureq::http::Method,
    url: &url::Url,
    headers: &[(String, String)],
    maximum_headers: usize,
    timeout: Duration,
    body: B,
) -> Result<ureq::http::Response<ureq::Body>, HttpFailure> {
    let mut builder = ureq::http::Request::builder()
        .method(method.clone())
        .uri(url.as_str());
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    builder
        .body(body)
        .map_err(|error| HttpFailure::Transport(error.to_string()))?
        .with_agent(agent)
        .configure()
        .http_status_as_error(false)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .max_response_header_size(maximum_headers)
        .timeout_global(Some(timeout))
        .run()
        .map_err(|error| map_ureq_error(&error, url.scheme() == "https"))
}

fn response_from_ureq(
    response: ureq::http::Response<ureq::Body>,
) -> Result<HttpResponse<ureq::Body>, HttpFailure> {
    let status = i64::from(response.status().as_u16());
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            value
                .to_str()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            HttpFailure::Transport(format!("invalid response header value: {error}"))
        })?;
    Ok(HttpResponse {
        status,
        headers,
        body: response.into_body(),
    })
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn should_drop_body(status: u16, method: &ureq::http::Method) -> bool {
    (status == 303 && method != ureq::http::Method::HEAD)
        || (matches!(status, 301 | 302) && method == ureq::http::Method::POST)
}

fn is_sensitive_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("authorization")
        || name.eq_ignore_ascii_case("cookie")
        || name.eq_ignore_ascii_case("proxy-authorization")
}

pub(super) fn perform(
    agent: &ureq::Agent,
    request: HttpRequest,
) -> Result<HttpResponse<Vec<u8>>, HttpFailure> {
    let maximum = request.maximum_body.unwrap_or(8 * 1024 * 1024);
    let response = open_response(agent, request)?;
    let mut body = Vec::new();
    let read_limit = maximum
        .checked_add(1)
        .ok_or(HttpFailure::BodyTooLarge(maximum))?;
    response
        .body
        .into_reader()
        .take(u64::try_from(read_limit).unwrap_or(u64::MAX))
        .read_to_end(&mut body)
        .map_err(|error| {
            let error = crate::handle::normalize_http_read_error(error);
            if error.kind() == std::io::ErrorKind::TimedOut {
                HttpFailure::Timeout(error.to_string())
            } else {
                HttpFailure::Transport(error.to_string())
            }
        })?;
    if body.len() > maximum {
        return Err(HttpFailure::BodyTooLarge(maximum));
    }
    Ok(HttpResponse {
        status: response.status,
        headers: response.headers,
        body,
    })
}

fn map_ureq_error(error: &ureq::Error, secure: bool) -> HttpFailure {
    match error {
        ureq::Error::Timeout(_) => HttpFailure::Timeout(error.to_string()),
        ureq::Error::TooManyRedirects | ureq::Error::RedirectFailed => {
            HttpFailure::Redirect(error.to_string())
        }
        ureq::Error::LargeResponseHeader(_, maximum) => HttpFailure::HeadersTooLarge(*maximum),
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) | ureq::Error::TlsRequired => {
            HttpFailure::Tls(error.to_string())
        }
        ureq::Error::Io(source) if secure && source.kind() == std::io::ErrorKind::InvalidData => {
            HttpFailure::Tls(error.to_string())
        }
        _ => HttpFailure::Transport(error.to_string()),
    }
}

fn positive_usize(value: i64, name: &str) -> Result<usize, RuntimeError> {
    if value <= 0 {
        return Err(RuntimeError::builtin(format!(
            "httpRequestRaw {name} must be positive"
        )));
    }
    usize::try_from(value).map_err(|_out_of_range| {
        RuntimeError::builtin(format!("httpRequestRaw {name} is too large"))
    })
}

fn expect_http_int(value: Value, name: &str) -> Result<i64, RuntimeError> {
    match value {
        Value::Int(value) => Ok(value),
        _ => Err(RuntimeError::builtin(format!(
            "httpRequestRaw {name} must be Int"
        ))),
    }
}

fn nonnegative_usize(value: i64, name: &str) -> Result<usize, RuntimeError> {
    usize::try_from(value).map_err(|_out_of_range| {
        RuntimeError::builtin(format!("httpRequestRaw {name} must be non-negative"))
    })
}

fn positive_u64(value: i64, name: &str) -> Result<u64, RuntimeError> {
    if value <= 0 {
        return Err(RuntimeError::builtin(format!(
            "httpRequestRaw {name} must be positive"
        )));
    }
    u64::try_from(value).map_err(|_out_of_range| {
        RuntimeError::builtin(format!("httpRequestRaw {name} is too large"))
    })
}

fn nonnegative_u32(value: i64, name: &str) -> Result<u32, RuntimeError> {
    u32::try_from(value).map_err(|_out_of_range| {
        RuntimeError::builtin(format!(
            "httpRequestRaw {name} must be between zero and {}",
            u32::MAX
        ))
    })
}
