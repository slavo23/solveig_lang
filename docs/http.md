# HTTP and HTTPS

`std.http` is the practical client layer above `std.net`. It owns URL and
header validation, request policy, response limits, status/header access, and
chunk-oriented body consumption. The VM owns blocking host execution and TLS.

## Safe request flow

Construct URLs and headers through their validating functions, choose explicit
limits, and treat non-2xx statuses as ordinary data:

```solveig
import std.http
import std.bytes

fetch : String -> IO std.http.HttpError String
fetch spelling =
    case std.http.urlFromString spelling of
        Err error => failIO error
        | Ok url => do
            response <- std.http.request
                (std.http.HttpLimits
                    65536
                    1048576
                    5000
                    (std.http.FollowRedirects 3))
                (std.http.get url)
            case std.http.responseText response of
                Ok text => pureIO text
                | Err error => failIO error
```

The four `HttpLimits` fields are maximum response-header bytes, maximum body
bytes, a whole-request timeout in milliseconds, and redirect policy. Header
bytes must be positive, body bytes non-negative, timeout positive, and redirect
count non-negative. `defaultHttpLimits` is 64 KiB, 8 MiB, 15 seconds, and five
redirects.

HTTP 4xx and 5xx responses are not transport errors. This lets applications
handle 304 caching, 404 absence, authentication responses, and rate limits by
status and headers without losing their bounded response bodies.

## Bodies and cleanup

The host validates content length and chunk framing, reads at most the body
limit plus one detection byte, then drops the host response before returning.
Only the bounded byte string crosses into the moving VM heap. This gives whole
response ownership a simple cleanup rule and prevents unbounded network input.

`bodyChunks size response` returns bounded slices. `foldBodyChunks` visits the
same slices without constructing a list:

```solveig
countBytes : std.http.Response -> Result std.http.HttpError Int
countBytes = std.http.foldBodyChunks
    16384
    (fun total chunk => total + std.bytes.bytesLength chunk)
    0
```

These APIs are incremental consumption of an already bounded body, not an
unbounded streaming escape hatch.

## Cancellation and timeouts

HTTP calls run on the existing blocking IO pool, so they do not stall Solveig
green-thread scheduling. `std.task.timeout` can cancel the waiting Solveig
thread promptly. A blocking host call already inside the transport is not
forcefully interrupted mid-system-call; it owns and drops its response no later
than the configured HTTP timeout. Keep timeouts finite and size request
concurrency to the four-worker host pool.

## HTTPS and caching

HTTPS uses Rustls with platform certificate verification. There is no API for
disabling verification. Redirects are bounded and failures are typed.

For ETag caching, retain the response's `ETag` header and send it later as
`If-None-Match`. Treat status 304 as a successful cache validation. Prefer one
bounded, overwritten cache file or a bounded key set; do not create a new file
on every request. `examples/release_radar` is the complete reference: its
default mode runs a deterministic local service, while `--live` explicitly
opts into the GitHub Releases API.
