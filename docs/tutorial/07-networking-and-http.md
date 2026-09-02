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
    response <- get url
    case responseText response of
        Right text => pureIO text
        | Left error => failIO error
```

`get` uses practical defaults: at most 64 KiB of response headers, an 8 MiB
buffered body, a 15-second whole-request timeout, and five redirects. Those
defaults protect the program without burdening the simple call site.

Use `getWith` only when the application needs headers, query parameters, or a
different policy:

```solveig
import std.time(milliseconds)

getWith
    (withHeader "Accept" "application/json" $
        withQuery [("kind", "release"), ("tag", "stable")] $
            withRedirectPolicy NoRedirects $
                withTimeout (milliseconds 5000) $
                    withMaximumBufferedBodyBytes 1048576 defaultOptions)
    url
```

`HttpOptions` is opaque and immutable: named transformations make each change
clear. Query names and values are host-encoded and may contain Unicode.
Explicit byte bounds belong at a trust boundary, not on every beginner example.

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

For a response that should be processed as it arrives, use `withResponse`.
Its callback receives status, headers, and a scoped `Stream` of binary chunks.
`withResponseWith options method url callback` selects policy; configure
`withChunkBytes` for the per-pull bound and `withMaximumResponseBytes` for an
optional cumulative quota. A stream fold can process the body without retaining
it; returning early closes without draining the rest. The stream cannot escape
its callback. See [streaming uploads and downloads](../http.md#streaming-uploads-and-downloads).

## Build non-GET requests

Each familiar verb has a `With` form for reusable options:

```solveig
import std.http(*)
import std.json(Json)

postJson : String -> Json -> IO HttpError Response
postJson url value =
    postWith
        (withHeader "Accept" "application/json" $ withJsonBody value defaultOptions)
        url
```

`withTextBody`, `withJsonBody`, and `withFormBody` choose useful content types;
an explicit header overrides the default. `withBody` sends strict bytes,
`withBuilderBody` avoids joining existing chunks, and `withFileBody` uploads a
file incrementally. The client owns framing headers such as `Content-Length`.
Use `request method url` only when code selects the method dynamically.

`requestStream` sends a demand-driven stream of binary chunks. A streamed body
is one-shot: a redirect that requires replay fails explicitly. Use
`withStreamResponse` when both upload and response should remain incremental.

HTTPS verifies certificates. The public API has no switch that disables
verification.

## Typed HTTP errors

`HttpError` keeps pure validation separate from host IO. It distinguishes URL,
header, option, transport, TLS, redirect, replay, request-file, response-bound,
UTF-8, managed-header, and unexpected-status failures. A function can retain
this precise type or translate it at an application boundary with `mapError`.
Every case has a useful `Show` representation.

## Concurrent requests

HTTP waiting does not block other runnable Solveig threads. A bounded pool can
preserve the precise HTTP error type:

```solveig
fetchStatus : String -> IO HttpError Int
fetchStatus url = do
    response <- get url
    pureIO (statusCode (responseStatus response))

fetchAll : [String] -> IO (ConcurrentError HttpError) [Int]
fetchAll = mapConcurrentlyBoundedOrdered 4 fetchStatus
```

The result follows URL input order. Use
`mapConcurrentlyBoundedCompletionOrder` when a UI or probe should receive the
same successful results in arrival order instead.

A token for an authenticated API should come from `std.env.getEnv` or
`requireEnv`, then be added as a header. Never put the secret directly in
source or print it. Authentication usually raises rate limits but does not
remove the service's usage policy; handle status and rate-limit headers.

## TCP clients

`withConnection host port body` owns an outgoing socket:

```solveig
import std.bytes(*)
import std.net(*)

echo : String -> Port -> String -> IO NetError String
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

`withListener port body` binds a listener. Pass `anyPort` to ask the
operating system for an available port; it returns
`(listener, actualPort)`. Validate externally supplied integers with
`portFromInt`. `accept` waits for a
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
4. Runs three `get` clients through `mapConcurrentlyBoundedOrdered`.
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
