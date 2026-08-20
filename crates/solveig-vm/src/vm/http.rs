//! Bounded HTTP request preparation and blocking host execution.
//! VM values are decoded on the scheduler thread; workers receive only owned
//! Rust data and return one bounded response value.

use super::*;
use crate::builtins::VmCtx;
use crate::heap::HeapObject;
use solveig_ast::ast::CtorRuntimeRole;
use std::io::Read;
use std::time::Duration;
use ureq::RequestExt;
use ureq::tls::{RootCerts, TlsConfig};

pub(super) struct HttpRequest {
    pub(super) method: String,
    pub(super) url: String,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: Option<Vec<u8>>,
    pub(super) maximum_headers: usize,
    pub(super) maximum_body: usize,
    pub(super) timeout: Duration,
    pub(super) maximum_redirects: u32,
}

pub(super) struct HttpResponse {
    pub(super) status: i64,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: Vec<u8>,
}

pub(super) enum HttpFailure {
    Transport(String),
    Tls(String),
    Timeout(String),
    Redirect(String),
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
        let (method, request_tail) = self.expect_http_pair(request)?;
        let (url, request_tail) = self.expect_http_pair(request_tail)?;
        let (headers, body) = self.expect_http_pair(request_tail)?;
        let (maximum_headers, limits_tail) = self.expect_http_pair(limits)?;
        let (maximum_body, limits_tail) = self.expect_http_pair(limits_tail)?;
        let (timeout_milliseconds, maximum_redirects) = self.expect_http_pair(limits_tail)?;
        let maximum_headers = expect_http_int(maximum_headers, "header limit")?;
        let maximum_body = expect_http_int(maximum_body, "body limit")?;
        let timeout_milliseconds = expect_http_int(timeout_milliseconds, "timeout")?;
        let maximum_redirects = expect_http_int(maximum_redirects, "redirect limit")?;
        let maximum_headers = positive_usize(maximum_headers, "header limit")?;
        let maximum_body = nonnegative_usize(maximum_body, "body limit")?;
        let timeout_milliseconds = positive_u64(timeout_milliseconds, "timeout")?;
        let maximum_redirects = nonnegative_u32(maximum_redirects, "redirect limit")?;
        Ok(HttpRequest {
            method: self.expect_string_value(method, builtin)?,
            url: self.expect_string_value(url, builtin)?,
            headers: self.expect_http_headers(headers)?,
            body: self.expect_optional_http_body(body)?,
            maximum_headers,
            maximum_body,
            timeout: Duration::from_millis(timeout_milliseconds),
            maximum_redirects,
        })
    }

    fn expect_http_pair(&mut self, value: Value) -> Result<(Value, Value), RuntimeError> {
        let builtin = "httpRequestRaw";
        let pair_tag =
            VmCtx::new(self).ctor_tag_with_arity(CtorRuntimeRole::PreludePair, 2, builtin)?;
        let Value::Obj(id) = value else {
            return Err(RuntimeError::builtin(
                "httpRequestRaw expected a Pair payload".to_string(),
            ));
        };
        match self.heap.get(id) {
            HeapObject::Data2 {
                tag,
                field0,
                field1,
            } if *tag == pair_tag => Ok((*field0, *field1)),
            _ => Err(RuntimeError::builtin(
                "httpRequestRaw expected a Pair payload".to_string(),
            )),
        }
    }

    fn expect_http_headers(
        &mut self,
        mut current: Value,
    ) -> Result<Vec<(String, String)>, RuntimeError> {
        let builtin = "httpRequestRaw";
        let nil_tag =
            VmCtx::new(self).ctor_tag_with_arity(CtorRuntimeRole::PreludeListNil, 0, builtin)?;
        let cons_tag =
            VmCtx::new(self).ctor_tag_with_arity(CtorRuntimeRole::PreludeListCons, 2, builtin)?;
        let pair_tag =
            VmCtx::new(self).ctor_tag_with_arity(CtorRuntimeRole::PreludePair, 2, builtin)?;
        let mut headers = Vec::new();

        loop {
            let Value::Obj(id) = current else {
                return Err(RuntimeError::builtin(
                    "httpRequestRaw expected a [Pair String String] header list".to_string(),
                ));
            };
            match self.heap.get(id) {
                HeapObject::Data0 { tag } if *tag == nil_tag => return Ok(headers),
                HeapObject::Data2 {
                    tag,
                    field0,
                    field1,
                } if *tag == cons_tag => {
                    let pair = *field0;
                    current = *field1;
                    let Value::Obj(pair_id) = pair else {
                        return Err(RuntimeError::builtin(
                            "httpRequestRaw expected Header pairs".to_string(),
                        ));
                    };
                    let (name, value) = match self.heap.get(pair_id) {
                        HeapObject::Data2 {
                            tag,
                            field0,
                            field1,
                        } if *tag == pair_tag => (*field0, *field1),
                        _ => {
                            return Err(RuntimeError::builtin(
                                "httpRequestRaw expected Header pairs".to_string(),
                            ));
                        }
                    };
                    headers.push((
                        self.expect_string_value(name, builtin)?,
                        self.expect_string_value(value, builtin)?,
                    ));
                }
                _ => {
                    return Err(RuntimeError::builtin(
                        "httpRequestRaw expected a [Pair String String] header list".to_string(),
                    ));
                }
            }
        }
    }

    fn expect_optional_http_body(&mut self, value: Value) -> Result<Option<Vec<u8>>, RuntimeError> {
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
                "httpRequestRaw expected Maybe Bytes".to_string(),
            ));
        };
        match self.heap.get(id) {
            HeapObject::Data0 { tag } if *tag == nothing => Ok(None),
            HeapObject::Data1 { tag, field } if *tag == just => {
                self.expect_bytes_value(*field, builtin).map(Some)
            }
            _ => Err(RuntimeError::builtin(
                "httpRequestRaw expected Maybe Bytes".to_string(),
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

pub(super) fn perform(
    agent: &ureq::Agent,
    request: HttpRequest,
) -> Result<HttpResponse, HttpFailure> {
    let secure = request.url.starts_with("https://");
    let method = ureq::http::Method::from_bytes(request.method.as_bytes())
        .map_err(|error| HttpFailure::Transport(error.to_string()))?;
    let mut builder = ureq::http::Request::builder()
        .method(method)
        .uri(&request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name, value);
    }
    let response = match request.body {
        Some(body) => builder
            .body(body)
            .map_err(|error| HttpFailure::Transport(error.to_string()))?
            .with_agent(agent)
            .configure()
            .http_status_as_error(false)
            .max_redirects(request.maximum_redirects)
            .max_redirects_will_error(true)
            .max_response_header_size(request.maximum_headers)
            .timeout_global(Some(request.timeout))
            .run(),
        None => builder
            .body(())
            .map_err(|error| HttpFailure::Transport(error.to_string()))?
            .with_agent(agent)
            .configure()
            .http_status_as_error(false)
            .max_redirects(request.maximum_redirects)
            .max_redirects_will_error(true)
            .max_response_header_size(request.maximum_headers)
            .timeout_global(Some(request.timeout))
            .run(),
    }
    .map_err(|error| map_ureq_error(&error, secure))?;

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
    let mut body = Vec::new();
    let read_limit = request
        .maximum_body
        .checked_add(1)
        .ok_or(HttpFailure::BodyTooLarge(request.maximum_body))?;
    response
        .into_body()
        .into_reader()
        .take(u64::try_from(read_limit).unwrap_or(u64::MAX))
        .read_to_end(&mut body)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::TimedOut {
                HttpFailure::Timeout(error.to_string())
            } else {
                HttpFailure::Transport(error.to_string())
            }
        })?;
    if body.len() > request.maximum_body {
        return Err(HttpFailure::BodyTooLarge(request.maximum_body));
    }
    Ok(HttpResponse {
        status,
        headers,
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
