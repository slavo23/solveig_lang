# HTTP and HTTPS

`std.http` is Solveig's high-level web client. The common API starts with a URL
string and a familiar verb: `get`, `post`, `put`, `patch`, `delete`, `head`, or
`options`. It validates request metadata, follows redirects, verifies HTTPS,
and returns a bounded `Response`. Resource policy is available through
`HttpOptions`, but it is not part of the simplest call.

## Common requests

```solveig
import std.http(*)

showPage : String -> IO HttpError String
showPage url = do
    result <- get url
    case responseText result of
        Right text => pureIO text
        | Left problem => failIO problem
```

4xx and 5xx statuses are ordinary responses. This preserves response headers
and error bodies for authentication, caching, rate-limit, and application
logic. Use `ensureSuccess` when a particular call should reject statuses from
400 upward.

`findHeader` compares names without ASCII case sensitivity. `responseBody`
returns binary `Bytes`; `responseText` performs strict UTF-8 decoding.

## Headers, query parameters, and bodies

Start from `defaultOptions`, apply only the settings a request needs, then use
the corresponding `With` verb:

```solveig
import std.http(*)

search : String -> String -> IO HttpError Response
search token term =
    getWith
        (withHeader "Authorization" ("Bearer " <> token) $
            withHeader "Accept" "application/json" $
                withQuery [("q", term), ("tag", "stable"), ("tag", "typed")] defaultOptions)
        "https://api.example.test/search"
```

Query and form pairs preserve order and duplicate keys. The host `url` library
performs UTF-8 percent encoding, so callers should supply semantic names and
values rather than constructing encoded query text themselves.

Body setters replace any earlier body:

- `withBody` sends strict binary `Bytes`.
- `withTextBody` sends UTF-8 and defaults to `text/plain; charset=utf-8`.
- `withJsonBody` renders `Json` and defaults to `application/json`.
- `withFormBody` encodes pairs as `application/x-www-form-urlencoded`.
- `withBuilderBody` sends retained `BytesBuilder` chunks without first joining
  them into one `Bytes` value.
- `withFileBody` opens and uploads a file incrementally in binary mode.
- `withoutBody` removes a configured body.

An explicit `Content-Type` header overrides a body setter's default. The client
owns `Host`, `Content-Length`, and `Transfer-Encoding`; trying to set one fails
with `ManagedRequestHeader` before network work.

For example:

```solveig
postJson : String -> Json -> IO HttpError Response
postJson url value =
    postWith
        (withHeader "Accept" "application/json" $ withJsonBody value defaultOptions)
        url
```

`request method url` and `requestWith options method url` cover code that
selects a method dynamically. `urlFromString` and `header` remain useful pure
validators, but normal callers do not need to stage a low-level request value.

## Streaming uploads and downloads

`requestStream method body url` sends a `Stream scope HttpError Bytes` and
buffers the bounded response. The transport pulls one non-empty chunk at a
time; empty chunks are skipped, no global generator or upload queue is used,
and a source failure stops the request.

Use `withResponse` when the response should be processed incrementally:

```solveig
import std.bytes(bytesLength)
import std.http(*)
import std.stream

downloadSize : String -> IO HttpError Int
downloadSize url =
    withResponse Get url (fun _ _ body =>
        std.stream.fold (fun total chunk => total + bytesLength chunk) 0 body)
```

`withStreamResponse` combines a streamed upload with a scoped streamed
response. Its rank-2 callback prevents the response stream from escaping the
connection lifetime. Each body pull is bounded and demand-driven. Returning
early closes without draining the unread suffix.

Every upload has one joined host owner thread because the underlying HTTP
transport is synchronous. The VM uses a one-slot rendezvous: each host demand
completes one Solveig IO action, and only then can the producer supply another
chunk. Success, typed failure, defects, interruption, and early callback return
all close the resource and join that owner. There is no detached thread or GC
finalizer.

`bodyChunks` and `foldBodyChunks` operate on an already-buffered `Response`.
They avoid another whole-body list, but are not network streaming APIs.

## Practical defaults and explicit policy

`defaultOptions` provides the policy most applications should start with:

- a 15-second whole-request deadline;
- up to five redirects;
- 64 KiB of response headers;
- at most 8 MiB when a response is buffered;
- 32 KiB per scoped response pull;
- no total-byte quota on a scoped response unless requested.

Policy is configured by meaningful names rather than positional integers:
`withTimeout`, `withRedirectPolicy`, `withReplayPolicy`,
`withMaximumHeaderBytes`, `withMaximumBufferedBodyBytes`,
`withMaximumResponseBytes`, and `withChunkBytes`. The byte bounds are advanced
trust-boundary controls; they do not appear in ordinary request examples.

```solveig
import std.http(*)
import std.time(seconds)

smallRequest : HttpOptions
smallRequest =
    withTimeout (seconds 5) $
        withRedirectPolicy NoRedirects $
            withMaximumBufferedBodyBytes 1048576 defaultOptions
```

Invalid options are typed `InvalidHttpOption` values. Header and chunk limits
must be positive; buffered and total response byte limits may be zero; timeout
must be positive; redirect counts cannot be negative.

## Redirect and replay semantics

`NoRedirects` returns the first redirect response. `FollowRedirects count`
bounds the number followed. Relative `Location` values are resolved with the
host URL library. Cross-origin redirects remove `Authorization`, `Cookie`, and
`Proxy-Authorization`.

`ReplayableBodies` permits a redirect to resend strict bytes, text, JSON, form,
builder chunks, or a file. Files are rewound, not copied into memory.
`NoReplay` rejects a redirect that would resend any body. Stream uploads are
always one-shot and fail with `BodyReplayError` when a 307 or 308 would require
replay. Standard POST-to-GET handling for 301, 302, and 303 drops the body and
its content headers.

## Timeouts, cancellation, and HTTPS

The timeout is one deadline across connection setup, redirects, upload, and
host response reads; redirects do not reset it. Time spent in a scoped callback
does not itself consume transport work, so wrap the whole action with
`std.task.timeout` when application processing also needs a deadline.

HTTP uses the existing blocking IO machinery, so a waiting request does not
stall other runnable Solveig threads. A system call already executing in the
host cannot always be interrupted immediately; cleanup waits for it to finish
or reach the request deadline. Keep request deadlines finite.

HTTPS uses Rustls with platform certificate verification. There is no API for
disabling verification. `HttpError` distinguishes validation, transport, TLS,
redirect, replay, request-file, response-bound, UTF-8, and unexpected-status
failures while retaining the relevant structured cause.

The [networking and HTTP tutorial](tutorial/07-networking-and-http.md) includes
a runnable loopback service and concurrent clients without requiring internet
access.
