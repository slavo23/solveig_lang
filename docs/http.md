# HTTP and HTTPS

`std.http` is the practical client layer above `std.net`. It owns URL and
header validation, request policy, response limits, status/header access, and
chunk-oriented body consumption. The VM owns blocking host execution and TLS.

## Safe request flow

The common GET path accepts a URL string directly and retains the bounded
response, including status and headers:

```solveig
import std.http(*)
import std.time

showPage : String -> IO std.http.HttpError String
showPage spelling = do
    response <- std.http.fetch spelling
    case std.http.responseText response of
        Right text => pureIO text
        | Left error => failIO error
```

`fetchWith limits headers spelling` keeps the same one-call shape when an
application needs tighter resource policy or request headers. Headers are
ordinary `(String, String)` values and are validated before network work:

```solveig
response <- std.http.fetchWith
    (std.http.withRedirectPolicy
        std.http.NoRedirects
        (std.http.withHttpTimeout
            (std.time.milliseconds 5000)
            (std.http.withMaximumBodyBytes
                1048576
                (std.http.withMaximumHeaderBytes 65536 std.http.defaultHttpLimits))))
    [ ("Accept", "application/json")
    , ("Authorization", "Bearer " <> token)
    ]
    "https://api.example.test/releases"
```

For other methods, bodies, or staged construction, the explicit layer remains:
validate with `urlFromString` and `header`, build with
`get`/`post`/`withHeader`/`withHeaders`/`withBody`/`withTextBody`, then call
`send` for default limits or `request` for explicit limits.

`HttpLimits` is opaque. Start from `defaultHttpLimits`, then use
`withMaximumHeaderBytes`, `withMaximumBodyBytes`, `withHttpTimeout`, and
`withRedirectPolicy`. The corresponding accessors make policy inspection
explicit. Header bytes must be positive, body bytes non-negative, timeout
positive, and redirect count non-negative. The default is 64 KiB, 8 MiB,
15 seconds, and five redirects.

HTTP 4xx and 5xx responses are not transport errors. This lets applications
handle 304 caching, 404 absence, authentication responses, and rate limits by
status and headers without losing their bounded response bodies.

## Strict bodies

The host validates content length and chunk framing, reads at most the body
limit plus one detection byte, then drops the host response before returning.
Only the bounded byte string crosses into the moving VM heap. This gives whole
response ownership a simple cleanup rule and prevents unbounded network input.

`bodyChunks size response` returns bounded slices. `foldBodyChunks` visits the
same slices without constructing a list:

```solveig
countBytes : std.http.Response -> Either std.http.HttpError Int
countBytes = std.http.foldBodyChunks
    16384
    (fun total chunk => total + std.bytes.bytesLength chunk)
    0
```

These APIs are incremental consumption of an already bounded body, not an
unbounded streaming escape hatch.

## Scoped downloads

`withResponse request callback` makes the status and headers available before
consuming the response body. Its callback receives a single-consumer
`Stream scope HttpError Bytes`. Use `withResponseWith limits chunkBytes request
callback` to choose the total response limit and the maximum bytes per pull.
Defaults are the ordinary HTTP limits and 32 KiB pulls.

```solveig
import std.bytes(bytesLength)
import std.http(HttpError, Request, withResponse)
import std.stream

downloadSize : Request -> IO HttpError Int
downloadSize requestValue =
    withResponse requestValue (fun _ _ body =>
        std.stream.fold (fun total bytes => total + bytesLength bytes) 0 body)
```

The fresh scope prevents returning the body or a transformed body from the
callback. Status, headers, and computed results can escape. Feed chunks to an
existing file writer or parser, or use `take` to stop early. Collecting chunks
in a `BytesBuilder` is explicitly whole-body storage, not bounded-memory use.

Each demand performs one bounded host read; no Solveig producer or queue reads
ahead. `ureq` still owns a bounded protocol buffer, which can include bytes
received with the headers. `maximumBodyBytes` is cumulative, not a per-chunk
limit, and an exhausted allowance uses one extra byte to distinguish EOF from
overflow. Early return does not promise that the unread remainder is valid or
within the total limit: it closes without inspecting that remainder.

Returning normally, raising a typed error or defect, and cancelling the
callback all close the body. Unread data is discarded, not drained. The shared
host client may reuse a connection only after a fully consumed valid response;
an unfinished response is dropped by the host library. There is no GC finalizer
and no new connection pool. Streams must be consumed once, in order.

Request bodies are still strict `Bytes` (or explicitly encoded text). This
download API does not add streamed uploads, automatic retries, or a replayable
body abstraction. Redirects use the existing request policy; file/stream upload
and replay policy remain a separate unfinished part of V3.

## Cancellation and timeouts

HTTP calls run on the existing blocking IO pool, so they do not stall Solveig
green-thread scheduling. `std.task.timeout` can cancel the waiting Solveig
thread promptly. A blocking host call already inside the transport is not
forcefully interrupted mid-system-call. For an acquired scoped body, close is
queued behind its current read; scope cleanup waits until that read completes
or reaches the original whole-request host deadline. Time spent in the callback
does not reset that deadline, although an idle callback itself is not timed out
by the host: use `std.task.timeout` for a deadline on the complete action.
Cancellation during header acquisition tombstones the request and closes any
late body on its owning worker. Keep timeouts finite and size active host IO
concurrency to the four-worker pool. Idle body scopes do not occupy workers.

## HTTPS and caching

HTTPS uses Rustls with platform certificate verification. There is no API for
disabling verification. Redirects are bounded and failures are typed.

For ETag caching, retain the response's `ETag` header and send it later as
`If-None-Match`. Treat status 304 as a successful cache validation. Prefer one
bounded, overwritten cache file or a bounded key set; do not create a new file
on every request.

The [networking and HTTP tutorial](tutorial/07-networking-and-http.md) includes
a runnable loopback service and concurrent clients without requiring internet
access.
