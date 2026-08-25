# 7. Networking and HTTP

Solveig has two networking layers:

- `std.net` exposes TCP connections and listeners as byte streams.
- `std.http` provides a validated, bounded HTTP/HTTPS client with convenient
  defaults.

Use `std.http` for web APIs. Use `std.net` for a custom protocol or when you are
implementing the server side yourself.

## A normal HTTP GET

The common path does not ask the caller to configure limits:

```solveig
import std.http(*)

downloadText : String -> IO HttpError String
downloadText url = do
    response <- fetch url
    case responseText response of
        Right text => pureIO text
        | Left error => failIO error
```

`fetch` performs a GET with `defaultHttpLimits`: at most 64 KiB of response
headers, an 8 MiB body, a 15-second whole-request timeout, and five redirects.
Those defaults protect the program without burdening the simple call site.

Use `fetchWith` only when the application needs a different policy or request
headers:

```solveig
fetchWith
    (HttpLimits 65536 1048576 5000 NoRedirects)
    [("Accept", "application/json")]
    url
```

The fields are maximum header bytes, maximum body bytes, timeout milliseconds,
and redirect policy. Explicit limits belong at a trust boundary, not on every
beginner example.

## Status, headers, and body

An HTTP 404 or 500 is a valid `Response`, not a transport failure. Inspect it:

```solveig
summarize : Response -> Either HttpError String
summarize response =
    let code = statusCode (responseStatus response)
    in case responseText response of
        Left error => Left error
        | Right body => Right ("status " <> show code <> ": " <> body)
```

`isSuccessful` recognizes the 2xx range. `findHeader` compares header names
case-insensitively. `responseBody` returns `Bytes`; `responseText` performs
strict UTF-8 decoding and can fail.

The complete response body is already bounded when the value reaches Solveig.
`bodyChunks` and `foldBodyChunks` let a consumer process slices without
constructing another full list, but they do not turn the client into an
unbounded streaming API.

## Build non-GET requests

For staged request construction:

```solveig
import std.http(*)

postJson : String -> String -> IO HttpError Response
postJson spelling body =
    case urlFromString spelling of
        Left error => failIO error
        | Right url =>
            case header "Content-Type" "application/json" of
                Left error => failIO error
                | Right contentType =>
                    send (withTextBody body (withHeader contentType (post url)))
```

`urlFromString` accepts absolute `http` and `https` URLs. Header constructors
validate names and values before network work begins. Builders include `get`,
`head`, `post`, `put`, `patch`, `delete`, `withHeader`, `withHeaders`,
`withTextBody`, and `withBody`. `send` uses the defaults; `request` accepts an
explicit `HttpLimits`.

HTTPS verifies certificates. The public API has no switch that disables
verification.

## Typed HTTP errors

`HttpError` keeps pure request validation separate from host IO:

```solveig
data HttpError =
    InvalidUrl String
  | InvalidHeaderName String
  | InvalidHeaderValue String
  | InvalidHttpLimit String Int
  | HttpIoError IOError
```

`HttpIoError` may wrap transport, TLS, timeout, redirect, or response-limit
errors from the `IOError` vocabulary. A function can retain this precise error
type or translate it at an application boundary with `mapError`.

## Concurrent requests

HTTP waiting does not block other runnable Solveig threads. For independent
requests that should share the ordinary `IOError` channel, translate errors and
use a bounded pool:

```solveig
httpAsIOError : HttpError -> IOError
httpAsIOError (HttpIoError ioError) = ioError
httpAsIOError (InvalidUrl text) = HttpTransportError text
httpAsIOError (InvalidHeaderName name) = HttpTransportError name
httpAsIOError (InvalidHeaderValue value) = HttpTransportError value
httpAsIOError (InvalidHttpLimit name value) = HttpTransportError name

fetchStatus : String -> Task Int
fetchStatus url = do
    response <- mapError httpAsIOError (fetch url)
    pureIO (statusCode (responseStatus response))

fetchAll : [String] -> Task [Int]
fetchAll = mapConcurrentlyBounded 4 fetchStatus
```

The result follows URL input order. If you need live completion order for a UI,
have workers send indexed results to a channel and let a collector display each
arrival before creating the final ordered structure.

A token for an authenticated API should come from `std.env.getEnv` or
`requireEnv`, then be added as a header. Never put the secret directly in
source or print it. Authentication usually raises rate limits but does not
remove the service's usage policy; handle status and rate-limit headers.

## TCP clients

`withConnection host port body` owns an outgoing socket:

```solveig
import std.bytes(*)
import std.net(*)

echo : String -> Int -> String -> Task String
echo host port message =
    withConnection host port (fun socket => do
        sendBytes socket (stringToBytes message)
        reply <- recvBytes socket 4096
        case bytesToString reply of
            Right text => pureIO text
            | Left error => failIO error)
```

TCP is a byte stream, not a message protocol. One `sendBytes` is not guaranteed
to correspond to one `recvBytes`. A real protocol must define framing—for
example, a delimiter or a length prefix—and loop until a complete frame is
available.

## TCP listeners

`withListener port body` binds a listener. Port `0` asks the operating system
for an available port and returns `(listener, actualPort)`. `accept` waits for a
connection. Each accepted socket must be closed, normally with `finally` or a
small ownership helper.

Solveig does not currently provide a high-level HTTP server framework. The
tutorial's local program writes the small HTTP response itself over TCP so the
client example remains hermetic.

## Complete local HTTP program

[`local_http`](code/local_http/) does all of the following in one runnable
project:

1. Binds a loopback listener on an automatically selected port.
2. Starts the server in a `withThread` scope.
3. Accepts three connections and scopes every connection handler.
4. Runs three `fetch` clients through `mapConcurrentlyBounded`.
5. Waits for all handlers through a typed channel.
6. Closes every socket, channel, listener, and child scope.

Run it without internet access:

```sh
solveig --project-root docs/tutorial/code/local_http
```

Expected output:

```text
responses: 3
{"message":"hello from Solveig"}
```

The full source is intentionally kept in the tutorial project rather than
duplicated here. Read it alongside this chapter: the `reply`,
`finishConnection`, `acceptBatch`, and `runClients` functions each own one
clear piece of the protocol.

## Network design checklist

Before calling a network program complete, answer these questions:

- What bounds concurrent requests?
- What bounds response size and time?
- Which status codes are normal application results?
- Who owns and closes each socket?
- How does cancellation wake blocked work?
- Does output show completion order or intentionally restore input order?
- Can tests use a loopback service instead of public internet?

Next: [Parsers and JSON](08-parsers-and-json.md).
