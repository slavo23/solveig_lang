//! Demand-driven HTTP response bodies in the existing worker-owned Handle table.
//! ureq owns RFC 9112 framing and the original request deadline; this layer
//! retains the total-byte bound across pulls without buffering the whole body.

use super::HandleFailure;
use std::io::{self, Read};

pub(super) struct HttpBody {
    // None records a terminal limit failure after dropping the transport.
    body: Option<ureq::Body>,
    remaining: usize,
    maximum: usize,
}

impl HttpBody {
    pub(super) fn new(body: ureq::Body, maximum: usize) -> Self {
        Self {
            body: Some(body),
            remaining: maximum,
            maximum,
        }
    }

    pub(super) fn read_http_chunk(&mut self, maximum: i64) -> Result<Vec<u8>, HandleFailure> {
        let maximum = usize::try_from(maximum)
            .ok()
            .filter(|size| *size > 0)
            .ok_or_else(|| {
                read_failure(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "HTTP chunk size must be positive",
                ))
            })?;
        // Only a subsequent pull probes beyond the total bound. Early consumers
        // do not demand EOF, and an exactly-sized body still succeeds at EOF.
        let body = self.body.as_mut().ok_or(HandleFailure::HttpBodyTooLarge {
            maximum: self.maximum,
        })?;
        let size = maximum.min(self.remaining.max(1));
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|source| read_failure(io::Error::new(io::ErrorKind::OutOfMemory, source)))?;
        bytes.resize(size, 0);
        let count = body.as_reader().read(&mut bytes).map_err(read_failure)?;
        if count > self.remaining {
            self.body = None;
            return Err(HandleFailure::HttpBodyTooLarge {
                maximum: self.maximum,
            });
        }
        self.remaining -= count;
        bytes.truncate(count);
        Ok(bytes)
    }
}

fn read_failure(source: io::Error) -> HandleFailure {
    HandleFailure::Read {
        label: "HTTP response body".to_string(),
        source: normalize_http_read_error(source),
    }
}

pub(crate) fn normalize_http_read_error(source: io::Error) -> io::Error {
    // Body readers wrap ureq's deadline error in io::ErrorKind::Other.
    // Preserve the portable timeout kind used by all other host operations.
    if matches!(
        source
            .get_ref()
            .and_then(|error| error.downcast_ref::<ureq::Error>()),
        Some(ureq::Error::Timeout(_))
    ) {
        io::Error::new(io::ErrorKind::TimedOut, source)
    } else {
        source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_deadlines_retain_the_portable_timeout_kind() {
        let source = ureq::Error::Timeout(ureq::Timeout::Global).into_io();
        assert_eq!(
            normalize_http_read_error(source).kind(),
            io::ErrorKind::TimedOut
        );
    }

    #[test]
    fn binary_pulls_preserve_the_bound_across_readers_and_eof() {
        let mut body = HttpBody::new(ureq::Body::builder().data(vec![0, 255, 128, 1]), 4);
        assert!(matches!(
            body.read_http_chunk(0),
            Err(HandleFailure::Read { .. })
        ));
        assert_eq!(body.read_http_chunk(2).ok(), Some(vec![0, 255]));
        assert_eq!(body.read_http_chunk(8).ok(), Some(vec![128, 1]));
        assert_eq!(body.read_http_chunk(8).ok(), Some(vec![]));
        assert_eq!(body.read_http_chunk(8).ok(), Some(vec![]));
    }

    #[test]
    fn overflow_is_probed_only_when_demanded_including_zero_limit() {
        let mut body = HttpBody::new(ureq::Body::builder().data(b"abcd".to_vec()), 3);
        assert_eq!(body.read_http_chunk(3).ok(), Some(b"abc".to_vec()));
        assert!(matches!(
            body.read_http_chunk(3),
            Err(HandleFailure::HttpBodyTooLarge { maximum: 3 })
        ));
        assert!(matches!(
            body.read_http_chunk(3),
            Err(HandleFailure::HttpBodyTooLarge { maximum: 3 })
        ));

        let mut empty = HttpBody::new(ureq::Body::builder().data(vec![]), 0);
        assert_eq!(empty.read_http_chunk(1).ok(), Some(vec![]));
        let mut nonempty = HttpBody::new(ureq::Body::builder().data(vec![42]), 0);
        assert!(matches!(
            nonempty.read_http_chunk(1),
            Err(HandleFailure::HttpBodyTooLarge { maximum: 0 })
        ));
    }
}
