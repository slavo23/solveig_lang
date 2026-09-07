# 7. Networking and HTTP

Solveig has three networking layers:

- `std.net` exposes TCP connections and listeners as byte streams.
- `std.tls` adds verified encryption to an outgoing byte stream.
- `std.http` provides a validated, bounded HTTP/HTTPS client with convenient
  defaults.

Use `std.http` for web APIs. Use `std.tls` for a custom encrypted client
protocol, and `std.net` for plaintext custom protocols or server-side TCP/UDP.

## Build the right mental model

The layers solve different problems; none is merely a differently named way to
send the same value:

```text
application value
    │  JSON, form data, or another application format
HTTP request and response
    │  method, URL, status, headers, and body framing
TLS connection (for HTTPS)
    │  encryption, certificate-chain verification, and server-name verification
TCP connection
    │  reliable ordered bytes with no message boundaries
IP endpoint
       resolved address plus a validated port
```

A normal `get "https://example.test/items"` asks `std.http` to own the whole
stack: validate the URL, resolve the host, connect, negotiate and verify TLS,
write a correctly framed request, parse the response, follow the configured
redirect policy, and return a bounded body. Application code should not
reproduce those steps unless it is implementing a non-HTTP protocol.

UDP sits beside TCP rather than above it. It preserves individual datagrams but
does not provide TCP's reliable ordered byte stream. TLS and HTTP in Solveig use
TCP.

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

This is the intended requests-style API: URL-only verbs are the common
surface, `Response` owns status/headers/body, and advanced policy remains in an
immutable `HttpOptions` value. There is no public raw socket tuple or framing
record to assemble for an ordinary web request.

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

The four response bounds answer different questions:

- `withMaximumHeaderBytes` limits response metadata received before the body.
- `withMaximumBufferedBodyBytes` limits verbs such as `get` that return a
  complete strict `Response`.
- `withChunkBytes` limits one pull from a scoped streaming response.
- `withMaximumResponseBytes` optionally limits the cumulative bytes pulled
  from that stream.

A per-pull chunk bound controls working memory but does not limit total
transfer. Conversely, a total quota does not describe how much one pull may
allocate. Most programs need neither override because the defaults already
bound strict requests; introduce one when the application's trust or storage
policy requires it.

### Put an API boundary around HTTP

Application code should not pass raw `Response` values through the whole
program. A small client function can own transport policy, status policy, and
JSON decoding, returning the domain value callers actually need. The local
tutorial project decodes `{"message":"hello from Solveig"}` with a direct
decoder:

```solveig
import std.http(*)
import std.json(Decoder, decodeJson, defaultJsonLimits, recordDecoder, requiredField, stringDecoder)
import std.parser(foldReply)

messageDecoder : Decoder () String
messageDecoder = recordDecoder $ requiredField "message" stringDecoder

decodeMessage : Bytes -> Either HostError String
decodeMessage bytes =
    foldReply
        (fun _ message _ => Right message)
        (fun _ _ parseError =>
            Left $ HostError InvalidArgument "decode message response" $ show parseError)
        (fun _ _ _ =>
            Left $ HostError InvalidArgument "decode message response" "incomplete JSON")
        (decodeJson defaultJsonLimits messageDecoder bytes)
```

`decodeJson` returns `ParseReply`, so all three outcomes are handled explicitly:
a value, a positioned decoding error, or a request for more input. Complete
strict bytes should not remain awaiting, but the branch keeps the boundary
total. A production library would normally retain a domain `ApiError` sum
instead of translating straight to `HostError`; this fixture uses one error
type so it can share a small `Task` entry point.

The request adds normal API metadata and checks status before decoding:

```solveig
httpAsHostError : HttpError -> HostError
httpAsHostError (HttpHostError hostError) = hostError
httpAsHostError error = HostError InvalidArgument "HTTP" $ show error

fetchMessage : String -> Task String
fetchMessage url = do
    response <- mapError httpAsHostError $
        getWith
            (withHeader "Accept" "application/json" $
                withQuery [("language", "solveig")] defaultOptions)
            url
    successful <-
        case ensureSuccess response of
            Left error => failIO $ httpAsHostError error
            | Right value => pureIO value
    case decodeMessage $ responseBody successful of
        Left error => failIO error
        | Right message => pureIO message
```

That order is deliberate: transport must succeed before there is a response;
status policy decides whether the body represents success; decoding then turns
the successful representation into the domain value. For APIs whose error
bodies have a useful schema, decode them in the non-success branch instead of
calling `ensureSuccess` and discarding that context.

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

This distinction matters in recovery logic:

- “The server returned 404 with a JSON explanation” is a completed HTTP
  exchange. The status, headers, and body are useful application data.
- “DNS failed,” “the certificate was rejected,” “the deadline expired,” or
  “the response exceeded policy” means no usable `Response` was produced and
  appears as `HttpError`.

Use `ensureSuccess` only where every status from 400 upward is genuinely
exceptional. It converts such a response to `UnexpectedStatus`; it does not
change whether the network exchange completed.

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

Strict and streamed responses express different ownership:

```text
get url
└── network resource closes before Response is returned
    └── bounded body bytes are ordinary immutable data

withResponse method url callback
└── network resource remains open only inside callback
    └── each next/fold demand may perform network IO
```

The rank-2 `scope` prevents the second stream from being stored in the callback
result and used after its connection closes. Returning early deliberately
drops unread bytes. If validation requires the entire body, consume and
validate it before returning.

A large download can be written incrementally instead of buffered as one
`Response`:

```solveig
import std.io(WriteMode, hPutBytes, withFile)
import std.stream(forEach)

writeHttpChunk : Handle -> Bytes -> IO HttpError ()
writeHttpChunk handle = mapError HttpHostError . hPutBytes handle

downloadFile : String -> String -> IO HttpError ()
downloadFile url destination =
    withResponse Get url $ fun status _ body =>
        if isSuccessful status
            then mapError HttpHostError $
                withFile destination WriteMode $ fun handle =>
                    mapError httpAsHostError $ forEach (writeHttpChunk handle) body
            else failIO $ UnexpectedStatus status
```

The scoped body drives one bounded network pull at a time and `hPutBytes`
preserves arbitrary binary data. The destination is opened only after a 2xx
status. If a later read fails, the partial file remains; a production downloader
should write to an application-owned `.part` path and rename only after success,
or deliberately retain it for a validated resume protocol. A chunk bound limits
working memory, while `withMaximumResponseBytes` supplies an independent total
download quota.

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

Domain encoders keep request construction just as typed as response decoding:

```solveig
import std.http(HttpError(..), Response, defaultOptions, postWith, withJsonBody)
import std.json(Encoder, encodeJsonValue, recordEncoder, requiredFieldEncoder, stringEncoder)

data NewUser = NewUser String String

userName : NewUser -> String
userName (NewUser name _) = name

userEmail : NewUser -> String
userEmail (NewUser _ email) = email

newUserEncoder : Encoder NewUser
newUserEncoder =
    recordEncoder
        [
            requiredFieldEncoder "name" userName stringEncoder,
            requiredFieldEncoder "email" userEmail stringEncoder
        ]

createUser : String -> NewUser -> IO HttpError Response
createUser url user =
    case encodeJsonValue newUserEncoder user of
        Left error => failIO $ RequestJsonError error
        | Right json => postWith (withJsonBody json defaultOptions) url
```

Use `putWith` for full replacement, `patchWith` for a partial update,
`deleteWith` for deletion, and `headWith` when only metadata is needed. Solveig
does not assign application semantics to those verbs: the API contract still
decides idempotency, validation, and which statuses are successful.

`withTextBody`, `withJsonBody`, and `withFormBody` choose useful content types;
an explicit header overrides the default. `withBody` sends strict bytes,
`withBuilderBody` avoids joining existing chunks, and `withFileBody` uploads a
file incrementally. The client owns framing headers such as `Content-Length`.
Use `request method url` only when code selects the method dynamically.

Body setters replace an earlier configured body rather than combining two
payloads. The client owns `Host`, `Content-Length`, and `Transfer-Encoding`;
rejecting caller-supplied versions prevents contradictory framing. Ordinary
headers remain semantic name/value pairs, and an explicit `Content-Type`
overrides the default selected by a text, JSON, or form body.

`requestStream` sends a demand-driven stream of binary chunks. A streamed body
is one-shot: a redirect that requires replay fails explicitly. Use
`withStreamResponse` when both upload and response should remain incremental.

Strict text, byte, JSON, form, builder, and file bodies are replayable under
the default policy because the client can produce them again; a file is rewound
rather than copied into memory. A stream has effects and a current position, so
silently running it twice would be unsound. `NoReplay` is available when the
application does not authorize resending even a repeatable body.

HTTPS verifies certificates. The public API has no switch that disables
verification.

## Typed HTTP errors

`HttpError` keeps pure validation separate from host IO. It distinguishes URL,
header, option, transport, TLS, redirect, replay, request-file, response-bound,
UTF-8, managed-header, and unexpected-status failures. A function can retain
this precise type or translate it at an application boundary with `mapError`.
Every case has a useful `Show` representation.

Keep `HttpError` in library functions that can use its structure. Translate it
only at an application boundary. Mapping everything immediately to a string or
generic `HostError` throws away whether retrying, changing credentials,
reducing a body, or fixing a URL could help. A retry policy must also consider
the method and whether the body can safely be repeated; a transport error alone
does not prove that the server observed no request.

### Authentication, pagination, retries, and rate limits

These policies belong above the transport call:

- Read a bearer token with `requireEnv`, add `Authorization: Bearer ...` through
  `withHeader`, and never log the configured options or secret. HTTPS protects
  the token in transit; certificate verification remains mandatory.
- Represent a page cursor as `Maybe String`. Add it with `withQuery`, decode the
  returned items and next cursor, and recurse until `Nothing` or an
  application-defined page/item limit. A page limit prevents a buggy service
  from producing an endless traversal.
- Retry only failures the application has classified as transient, only for
  methods and bodies safe to repeat, and with a finite attempt count. A timeout
  or disconnected response does not prove the server failed to process a POST.
  An idempotency key supported by the service is an application protocol, not a
  guarantee supplied by `std.http`.
- Treat `429` and service-specific rate-limit headers as normal response policy.
  Bound concurrent calls, honor the server's delay when trustworthy, and cap
  the total retry window. More workers can reduce throughput when they create
  throttling.

Do not implement retry as unbounded recursion around `get`. A sound loop carries
at least the remaining attempts and a `Duration` backoff, and it keeps the
original typed error or final response when the budget is exhausted. Test it
against a loopback fixture with a scripted sequence such as `503, 503, 200`;
do not make a unit test depend on a public service.

For example, this policy retries only completed `GET` responses with two
explicitly transient statuses; transport failures propagate unchanged:

```solveig
import std.time(Duration, sleep)

retryableStatus : Status -> Bool
retryableStatus status =
    let code = statusCode status
    in or (code == 429) (code == 503)

getWithRetries : Int -> Duration -> String -> IO HttpError Response
getWithRetries remaining delay url = do
    response <- get url
    if and (remaining > 0) (retryableStatus $ responseStatus response)
        then do
            mapError HttpHostError $ sleep delay
            getWithRetries (remaining - 1) delay url
        else pureIO response
```

This is intentionally a narrow example, not a universal retry helper. A real
client may parse `Retry-After`, add a total deadline, and randomize backoff, but
those are service policies. Extending it to POST without an idempotency contract
would change correctness, not merely performance.

## Concurrent requests

HTTP waiting does not block other runnable Solveig threads. A bounded pool can
preserve the precise HTTP error type:

```solveig
fetchStatus : String -> IO HttpError Int
fetchStatus url = do
    response <- get url
    pureIO $ statusCode $ responseStatus response

fetchAll : [String] -> IO (ConcurrentError HttpError) [Int]
fetchAll = mapConcurrentlyBoundedOrdered 4 fetchStatus
```

The result follows URL input order. Use
`mapConcurrentlyBoundedCompletionOrder` when a UI or probe should receive the
same successful results in arrival order instead.

The bound limits active worker actions created by this call; it is not a
process-wide HTTP connection limit. Choose it from the remote service policy,
your descriptor budget, and measurements. A large bound can worsen latency by
creating queueing and rate-limit failures. Structured failure cancels the
remaining workers and waits for their cleanup before `fetchAll` returns.

A token for an authenticated API should come from `std.env.getEnv` or
`requireEnv`, then be added as a header. Never put the secret directly in
source or print it. Authentication usually raises rate limits but does not
remove the service's usage policy; handle status and rate-limit headers.

## TCP clients

A port is a 16-bit service number, represented by opaque `Port` rather than an
ambiguous `Int`. `portFromInt` validates external data; `anyPort` is meaningful
only when binding and asks the host to choose an available port. `IpAddress` is
produced by `resolveHost`, and `Endpoint` pairs one resolved address with a
port.

`withTcpConnection` performs resolution and tries returned addresses in order
until one connects. DNS runs on the bounded host worker pool, so it does not
freeze the green-thread scheduler. Resolution and connection failure remain
`NetError`, with host context retained.

`withTcpConnection host port body` resolves the hostname and owns an outgoing
connection in a fresh type-level scope:

```solveig
import std.bytes(*)
import std.net(*)

echo : String -> Port -> String -> IO NetError String
echo host port message =
    withTcpConnection host port (fun connection => do
        sendTcpBytes connection (stringToBytes message)
        reply <- receiveTcpBytes connection 4096
        case bytesToString reply of
            Right text => pureIO text
            | Left error => failIO (NetHostError "decode echo" $
                HostError InvalidArgument "bytesToString" (show error)))
```

TCP is a byte stream, not a message protocol. One `sendTcpBytes` is not
guaranteed to correspond to one `receiveTcpBytes`. A real protocol must define
framing—for example, a delimiter or a length prefix—and loop until a complete
frame is available. `tcpBytes connection chunkSize` supplies the same bounded,
demand-driven loop as a scoped `Stream`; it performs no read-ahead.

For a fixed-size frame, the loop requests only the missing bytes:

```solveig
receiveExactly : forall scope. TcpConnection scope -> Int -> Bytes -> IO NetError Bytes
receiveExactly connection wanted accumulated =
    let missing = wanted - bytesLength accumulated
    in if missing <= 0
        then pureIO accumulated
        else do
            chunk <- receiveTcpBytes connection missing
            if bytesLength chunk == 0
                then failIO
                    (NetHostError "fixed-size frame" $
                        HostError EndOfFile "receiveExactly" "peer closed during a frame")
                else receiveExactly connection wanted (bytesAppend accumulated chunk)
```

Production protocols must validate `wanted` before allocation and choose a
maximum frame size. Delimiter-based protocols need an analogous maximum, or a
peer that never sends the delimiter can force unbounded accumulation. Stream
chunk boundaries are transport events, never application records.

## Raw TLS clients

For HTTP and HTTPS, keep using `std.http`: it owns URLs, headers, redirects,
status codes, and practical limits. `std.tls` is for a non-HTTP protocol that
needs a verified encrypted byte stream:

```solveig
import std.bytes(stringToBytes)
import std.net(Port)
import std.tls(*)

sendGreeting : String -> Port -> IO TlsError Bytes
sendGreeting host port =
    withTlsConnection host port $ fun connection => do
        sendTlsBytes connection (stringToBytes "hello\n")
        receiveTlsBytes connection 4096
```

The default verifies both the certificate chain and `host` through the
operating system's trust policy. Advanced callers can build immutable options:

```solveig
options : Bytes -> TlsOptions
options certificate =
    withTlsServerName "service.example" $
        withTrustedCertificatesPem certificate defaultTlsOptions
```

The PEM certificate set replaces platform trust and is intended for private
PKI and hermetic tests. A name override changes the DNS name that must appear
in the certificate; it does not weaken verification. Solveig intentionally
offers no “accept any certificate” switch.

TLS protects the connection only after a successful handshake. Certificate
chain verification answers whether a configured trust root vouches for the
certificate; server-name verification answers whether that certificate is for
the host the program intended to reach. Encryption without both checks can
protect traffic to the wrong peer, which is why the public API never exposes an
insecure convenience flag.

As with TCP, TLS does not infer application-message framing. `receiveTlsBytes`
requires a positive maximum and returns one available plaintext chunk;
`tlsBytes` repeats that operation only when demanded. A clean peer shutdown is
empty bytes. A TCP EOF without TLS `close_notify` is rejected as truncated.
The scoped connection and stream cannot escape `withTlsConnection` or
`withTlsConnectionWith`.

## TCP listeners

`withTcpListener port body` binds IPv4 loopback. Pass `anyPort` to ask the
operating system for an available port; the callback receives the listener and
actual port separately. Validate externally supplied integers with
`portFromInt`. `withAcceptedTcpConnection` waits for one client and introduces
a nested connection scope. Both resources close through `bracket` on normal
return, typed failure, defect, cancellation, and timeout; neither can escape its
callback.

The listener scope and each accepted-connection scope are different lifetimes.
A server may keep accepting after one connection closes, but an accepted
connection cannot escape its own callback. The current API is a low-level
loopback building block, not a public-internet HTTP server framework.

That limitation matters for backend design. The current high-level HTTP API is
a client. It covers the ordinary work of calling downstream services—verbs,
query encoding, authentication headers, JSON/form/file bodies, redirects,
deadlines, bounded and streamed responses, status inspection, and concurrent
fan-out—but it does not provide routing, HTTP request parsing, response writers,
middleware, keep-alive management, or a production accept loop. Do not build a
public web service by copying the fixed TCP fixture below. Until a scoped HTTP
server API exists, use Solveig for clients, workers, scheduled jobs, and custom
bounded protocols, or place a deliberately designed host server boundary in
front of it.

## UDP datagrams

Use `withUdpSocket anyPort body` for connectionless messages. Unlike TCP, UDP
preserves message boundaries:

```solveig
sendDatagram sender "localhost" receiverPort (stringToBytes "ping")
message <- receiveDatagram receiver 4096
let senderEndpoint = datagramSource message in
pureIO (datagramBytes message)
```

`sendDatagramTo` accepts an already resolved `Endpoint`. `receiveDatagram`
returns one complete payload and source endpoint. Its explicit bound must be
between 1 and 65,535 bytes; a larger message is consumed and reported as an
error rather than silently truncated. `datagrams` exposes repeated receives as
a scoped stream, one demand per message.

UDP does not promise delivery, uniqueness, or ordering. Applications that need
acknowledgements, retransmission, deduplication, congestion control, or
fragmentation policy must define them at the protocol layer—or choose TCP when
its reliable ordered stream is the required abstraction. The receive bound
protects one datagram, while UDP's lack of EOF means the owning scope, a
failure, or interruption must terminate a receive loop.

Solveig does not currently provide a high-level HTTP server framework. The
tutorial's local program writes the small HTTP response itself over TCP so the
client example remains hermetic.

## Complete local HTTP program

[`local_http`](code/local_http/) does all of the following in one runnable
project:

1. Binds a loopback listener on an automatically selected port.
2. Starts the server in a `withThread` scope.
3. Accepts three connections and scopes every connection.
4. Runs three typed, JSON-decoding clients through
   `mapConcurrentlyBoundedOrdered`.
5. Closes the listener and every accepted connection through rank-2 scopes.
6. Closes every connection, listener, worker, and child scope.

Run it without internet access:

```sh
solveig --project-root docs/tutorial/code/local_http
```

Expected output:

```text
responses: 3
hello from Solveig
```

The full source is intentionally kept in the tutorial project rather than
duplicated here. Read it alongside this chapter: the `reply`,
`serveBatch`, `runClients`, and `runFixture` functions each own one clear piece
of the protocol.

The program is deliberately a fixture, not a miniature reusable HTTP server:

- `fixtureResponse` constructs one fixed response with an exact
  `Content-Length`.
- `reply` waits until the client has sent some request bytes, then returns the
  fixed response; it does not pretend that one TCP read contains a complete
  general HTTP request.
- `serveBatch` accepts exactly three connections, so its recursion has an
  explicit termination condition.
- `fetchMessage` uses `getWith`, safely encoded query data, an `Accept` header,
  explicit status policy, and a direct JSON decoder.
- `runClients` uses a bound of three. Ordered collection makes output
  deterministic even if requests finish differently.
- `runFixture` owns the server through `withThread`, and `main` owns the
  listener through `withTcpListener`. Leaving either lexical scope performs
  cleanup before returning.

This shape keeps the test hermetic: it binds only loopback, asks the host for an
ephemeral port, uses no DNS beyond the numeric loopback address, and needs no
public internet service or credential.

## Network design checklist

Before calling a network program complete, answer these questions:

- What bounds concurrent requests?
- What bounds response size and time?
- Which status codes are normal application results?
- Who owns and closes each socket?
- How does cancellation wake blocked work?
- Does output show completion order or intentionally restore input order?
- Can tests use a loopback service instead of public internet?
- Is a TCP/TLS chunk being mistaken for an application message?
- Can a redirect or retry repeat this method and body safely?
- Does streamed validation finish before the scoped response is closed?

Next: [Parsers and JSON](08-parsers-and-json.md).
