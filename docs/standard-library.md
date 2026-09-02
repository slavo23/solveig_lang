# Solveig Standard Library

The standard library is shipped as the ordinary `std` package. Unlike the
Prelude, its modules are never imported automatically. This keeps application
namespaces predictable and makes dependencies visible at the top of each
source file.

Applications do not list `std` in `[dependencies]`; the compiler loads its
shipped package automatically. See [Packages](packages.md) for compiler-library
discovery, package manifests, and the Prelude/std boundary.

## Module map

| Module | Purpose |
|---|---|
| `std.list` | Stack-safe finite List sizing, slicing, zipping, reversal, and ranges |
| `std.vector` | Persistent indexed sequences with logarithmic updates |
| `std.map` | Persistent ordered key-value maps |
| `std.set` | Persistent ordered sets |
| `std.hashmap` | Persistent hash key-value maps with expected constant-time access |
| `std.hashset` | Persistent hash sets with expected constant-time membership |
| `std.queue` | Persistent FIFO queues |
| `std.text` | Packed UTF-8 scans, explicit scalar/byte operations, and linear text construction |
| `std.bytes` | Strict/replacement UTF-8 decoding, packed byte scans, and linear binary construction |
| `std.stream` | Strict pull streams, scoped resource lifetimes, bounded collection, and finite adapters |
| `std.parser` | Generic parser combinators with structured errors and streaming input |
| `std.json` | Bounded byte-level JSON, exact numbers, path-aware decoders, and symmetric encoders |
| `std.http` | Bounded HTTP/HTTPS requests, verified TLS, and scoped response-body streams |
| `std.io` | Handles, buffering, and whole-file IO |
| `std.fs` | Directory, path, and file-metadata operations |
| `std.env` | Arguments, environment lookup, bounded dotenv loading, and program identity |
| `std.process` | Child processes and structured program exit |
| `std.channel` | Typed channel creation and communication |
| `std.thread` | Raw lightweight-thread creation and interruption |
| `std.task` | Cleanup, masking, structured concurrency, and bounded concurrent mapping |
| `std.testing` | Assertions, explicit deterministic generators, property checks, and resource-aware checks |
| `std.time` | Sleeping, clock readings, and monotonic measurement |
| `std.random` | Explicit deterministic pseudorandom state and separate host-secure bytes |
| `std.net` | TCP sockets and hostname resolution |

## Lists and text

```solveig
import std.list(length, isEmpty, take, drop, zip, reverse, range, rangeInclusive)
import std.text(
    byteLength, byteSlice, charAt, lines, slice, splitOnce, stringToChars,
    charsToString, intToString
)
```

`std.list` provides `length`, `isEmpty`, `take`, `drop`, `takeWhile`,
`dropWhile`, `zip`, `zipWith`, `reverse`, half-open integer `range`, and
explicit `rangeInclusive`. These are separate from the universally imported
traversal operations such as `map` and the generic `Foldable` methods `foldl`
and `foldr`. `take` treats a non-positive count as empty; `drop` treats it as
zero. Public finite List transformations—including `map`, `filter`, `append`,
`concat`, `take`, `takeWhile`, and `zipWith`—are stack-safe. Operations that
must preserve order use a temporary reversed list rather than consuming the VM
call stack.

`std.text.length`, `charAt`, and `slice` count Unicode scalar values.
`byteLength` counts UTF-8 bytes, and `byteSlice` succeeds only when both byte
endpoints are scalar boundaries. A displayed grapheme may contain several
scalar values: for example, `std.text.length "é"` is `2`. The library does not
label scalar indexing as grapheme indexing.

`startsWith`, `trimAscii`, `lines`, and `splitOnce` scan the packed UTF-8
representation without first constructing `[Char]`. `lines` removes newline
separators, preserves interior empty lines, and does not invent an empty line
after a trailing newline.

`TextBuilder` avoids quadratic repeated `String` concatenation. Convert chunks
with `textBuilder` or `charBuilder`, combine builders using `(<>)` or
`appendTextBuilder`, and call `buildText` once. Combining is constant time;
building is stack-safe and linear in the result's UTF-8 bytes.

## Persistent collections

```solveig
import std.vector(*)
import std.map(*)
import std.set(*)
import std.hashmap
import std.hashset
import std.queue(*)
```

`Vector` is a persistent height-balanced sequence tree. `length` is constant
time; `index`, `update`, and `snoc` are logarithmic; `fromList` and `toList`
are linear. An update returns `Nothing` outside the vector and shares all
unchanged branches:

```solveig
numbers : std.vector.Vector Int
numbers = std.vector.fromList [10, 20, 30, 40]

changed : Maybe (std.vector.Vector Int)
changed = std.vector.update numbers 2 99
```

This is an immutable functional vector, not a contiguous mutable array. Its
tree representation provides predictable persistent updates with structural
sharing.

`Map` and `Set` use persistent AVL trees and therefore require `Ord` keys or
elements. Lookup, insertion, and deletion are logarithmic; `size` is constant
time; ordered traversal is linear and deterministic. `Map.fromList` consumes
ordinary `(key, value)` tuples and keeps the last value for a duplicate key.
`Set.fromList` discards duplicates.

```solveig
phoneBook : std.map.Map String String
phoneBook =
    std.map.fromList
        [ ("Ada", "+44")
        , ("Linus", "+358")
        ]

names : [String]
names = std.map.keys phoneBook
```

Use `HashMap` or `HashSet` when keys have `Hashable` and equality but ordered
traversal is not the primary operation. They use immutable bitmap-indexed
32-way hash tries. Dispersed lookup, insertion, and deletion have expected
constant cost, with at most thirteen trie levels for an `Int` hash. Updates
copy only the search spine and retain every untouched branch. A deliberately
bad `Hashable` instance can put several distinct keys in one full-hash
collision bucket; operations on that bucket are linear in its size.

```solveig
import std.hashmap

scores : std.hashmap.HashMap String Int
scores = std.hashmap.fromList [("Ada", 10), ("Grace", 12), ("Ada", 15)]

adaScore : Maybe Int
adaScore = std.hashmap.lookup "Ada" scores
```

`HashMap.toList`, `keys`, `values`, `HashSet.toList`, and their `Foldable`
instances use defined trie order: ascending hash fragments and insertion order
within a complete-hash collision bucket. They do not promise key order. Use
`toAscList` when an `Ord` instance is available and sorted output matters.
`Show` also uses ascending order, so equal values render identically regardless
of insertion history. `Hashable` is order-independent and remains available
without `Ord`; equal maps and sets therefore always produce equal salted
hashes.

All collection types are opaque. Use their public constructors and operations
so cached sizes, tree balance, hash fragments, and queue normalization remain
valid. `Functor`, `Foldable`, `Traversable`, `Eq`, `Show`, and `Hashable` are
provided where they preserve the collection abstraction. In particular,
`HashSet` is not a `Functor`: changing an element may change its hash and must
therefore be an explicit rebuild.

`Queue` uses a front list and a reversed rear list. `enqueue` and `dequeue`
are amortized constant time, `length` is constant time, and `toList` is linear.
`dequeue` returns both the oldest value and the persistent remainder:

```solveig
next : Maybe (String, std.queue.Queue String)
next =
    std.queue.dequeue
        (std.queue.enqueue (std.queue.enqueue std.queue.empty "first") "second")
```

## Bytes

```solveig
import std.bytes(*)
```

```solveig
stringToBytes : String -> Bytes
bytesToString : Bytes -> Either Utf8Error String
bytesLength : Bytes -> Int
bytesIndex : Bytes -> Int -> Maybe Int
bytesSlice : Bytes -> Int -> Int -> Bytes
bytesAppend : Bytes -> Bytes -> Bytes
bytesToStringReplacing : Bytes -> String
```

`String` is valid UTF-8; `Bytes` preserves arbitrary data. Decoding therefore
returns `Either` instead of silently replacing invalid input.
`bytesToStringReplacing` is the explicitly named alternative that substitutes
U+FFFD for malformed sequences.

The bulk `bytesSpanAny`, `bytesFindAny`, `bytesFindBelow`, and
`bytesSpanRange` operations scan without constructing a `Maybe` for each byte.
`bytesSliceToString` validates one slice without first allocating another
`Bytes` value. These operations are useful when implementing byte-oriented
formats; ordinary callers should prefer the higher-level parser for that
format.

`BytesBuilder` provides the binary counterpart to `TextBuilder`. Wrap chunks
with `bytesBuilder`, combine them using `(<>)` or `appendBytesBuilder`, and call
`buildBytes` once. This retains chunk values while building the plan, then
performs one stack-safe linear packed construction.

## Strict streams

```solveig
import std.stream(*)
```

`Stream scope e a` is the common strict pull protocol for incremental values.
`next` performs one pull and returns `StreamDone` or `StreamYield value rest`
inside `IO e`; it does not run a producer ahead of demand. Always continue with
the returned `rest` value. Resource-backed streams are single-consumer and must
not be pulled repeatedly or concurrently from the same stream value.

`map`, `filter`, `take`, `chunksOf`, `fold`, `forEach`, `toListLimited`, and
`mapError` preserve stream order. `take` performs no pull beyond its count,
`chunksOf size` buffers at most `size` values, and bounded collection retains at
most its declared limit while performing one extra pull only to detect
overflow. `fromList`,
`fromText`, and `fromBytes` adapt finite packed values without first copying
them to another representation.

Resource producers use a rank-2 callback. For example,
`withFileByteStream chunkSize path callback` supplies
`forall scope. Stream scope HostError Bytes` to the callback and closes the
file when it returns, fails, defects, or is interrupted. The fresh scope cannot
appear in the callback result, so the stream cannot outlive its file. See the
[strict-stream guide](streaming.md) for the pull protocol, bounded-memory rules,
early release, and complete examples.

## Parser combinators

```solveig
import std.parser(*)
import std.text(*)
```

`Parser input customError value` is generic over the cursor it consumes, the
domain errors a grammar can attach, and its successful result. An
`InputOps input token` value supplies token advancement, positions,
checkpoint restoration, and diagnostic rendering. The module includes ready
adapters for complete lists, Unicode text, immutable bytes, and persistent
feedable token streams; user-defined checkpointable inputs use the same
combinators.

```solveig
word : std.parser.Parser std.parser.TextInput () String
word =
    std.parser.mapResult std.text.charsToString
        (std.parser.some std.parser.textInputOps
            (std.parser.satisfy std.parser.textInputOps "letter"
                (fun character =>
                    if character >= 'a' then character <= 'z' else False)))
```

Build grammars from `satisfy`, `token`, and `tokens`; combine them with
`mapResult`, `applyParser`, `andThen`, `choice`, `many`, `optional`, and
`between`; then use `parseText`, `parseBytes`, or `parseTokenList` for the
built-in complete inputs, or `runComplete` for a custom cursor. Choice
retries only failures that consumed nothing. Put `attempt` narrowly around a
branch with an ambiguous consumed prefix, and put `cut` after a prefix that
unambiguously selects a branch. `defer` supports non-left-recursive recursive
grammars in Solveig's strict evaluation model.

Because `Parser input customError` implements `Functor`, `Applicative`, and
`Monad`, ordinary Prelude syntax works: use `<$>` and `<*>` for static grammar
structure, `<*` and `*>` for delimiters, and `do` or `>>=` only when a parsed
value selects later parsing. `parser <?> "name"` replaces the expected item on
an empty failure. Choice remains `choice ops left right` because correct
incremental backtracking needs the explicit input adapter.

`parseStream describe parser chunks` bridges the same pure engine to a
`Stream scope e [token]`. It pulls only when the parser is awaiting input,
preserves upstream failures as `IO e`, and returns grammar rejection as
`Either (ParseError custom) value`. That result shape composes directly inside
rank-2 resource callbacks; neither their scope nor their typed errors are
erased. The feedable cursor retains all supplied tokens until parsing ends;
neither incremental pulling nor `cut` promises bounded parser memory. There is
no second stream protocol. `mapCustomError` transforms application errors while
preserving positions, expectations, consumption, and suspension.

Every public parser operation has an API doc comment describing its behavior,
including failure, consumption, backtracking, and streaming details where they
apply. In an editor, hover the operation you are using; the method documentation
complements the [parser tutorial](tutorial/08-parsers-and-json.md).

## JSON

```solveig
import std.bytes(*)
import std.json(*)
import std.parser(*)
import std.vector(*)

reply : std.parser.ParseReply std.parser.ByteInput std.json.JsonError std.json.Json
reply =
    std.json.parseJson std.json.defaultJsonLimits
        (std.bytes.stringToBytes "{\"ready\":true,\"items\":[1,2,3]}")
```

`std.json` parses RFC 8259 bytes with explicit limits for total input, nesting,
string tokens, container size, and number tokens. Strings validate UTF-8,
escapes, control characters, and Unicode surrogate pairs. Duplicate object
names are rejected. Arrays use persistent `Vector`; objects use ordered `Map`,
so rendering is deterministic regardless of insertion order.

`JsonNumber` retains a validated decimal spelling instead of rounding through
`Float`. Construct it with `jsonNumberFromInt`, `jsonNumberFromString`, or
`jsonNumberFromFloat`; the float constructor rejects NaN and infinities.
Rendering revalidates even a directly constructed `JsonNumber`.
`jsonNumberToInt` performs exact decimal/exponent conversion and returns
`Nothing` for a fractional mathematical value or signed-`Int` overflow.

Use `parseJson` when the whole `Json` tree is useful. Use `decodeJson` with
`stringDecoder`, `boolDecoder`, `numberDecoder`, `nullDecoder`,
`arrayDecoder`, `objectDecoder`, or `nullableDecoder` when the requested value
can be built directly. For example, this creates a vector of strings without
first allocating a vector of `JsonString` nodes:

```solveig
names : std.parser.ParseReply std.parser.ByteInput (std.json.DecodeError ())
    (std.vector.Vector String)
names =
    std.json.decodeJson std.json.defaultJsonLimits
        (std.json.arrayDecoder std.json.stringDecoder)
        (std.bytes.stringToBytes "[\"Ada\",\"Grace\"]")
```

Both entry points require complete input apart from JSON whitespace and return
the same structured `std.parser.ParseReply`. `parseJson` reports `JsonError`;
`decodeJson` reports `DecodeError e`, separating `JsonDecodingError JsonError`
from `CustomDecodeError path e`. Both support `show` (the latter when `e` does).
Use `Decoder () a` when no application validation is needed. `validateDecoder`
accepts `a -> Either e b` and keeps failures as typed values at their field/index
path; `mapDecoderError` maps only application errors, leaving JSON errors intact.

For heterogeneous objects, combine `requiredField`, `optionalField`, and
`optionalFieldWithDefault` applicatively and finish with `recordDecoder`.
Missing is distinct from present null or malformed data. Nested failures carry
paths such as `$["users"][2]["name"]` with zero-based array indices; unknown fields are ignored.
`taggedDecoder` selects record-shaped variants through a required string tag.
These composition boundaries reuse one bounded `Json` value, while primitive,
homogeneous-array, and homogeneous-object decoding remains direct.

The symmetric `Encoder` surface includes primitive, array, object, nullable,
record, and tagged-sum builders. Record encoders use named field projections;
an optional `Nothing` is omitted, not encoded as null. Tagged sums use
`variantEncoder` projections and `taggedEncoder`. `encodeJson` applies explicit
limits and deterministic rendering, while `encodeJsonValue` returns `Json`
when another format-independent transformation needs it.
Value encoding constructs strict `Json` data; final rendering enforces limits
and validates even forged number spellings. `decodeJsonValue` traverses an
already-built tree and does not apply byte-input limits retroactively.

## Handles and files

```solveig
import std.io(*)
```

`std.io` owns the low-level `String`-path boundary: `IOMode`, `BufferMode`, the
standard handles, incremental handle operations, `withFile`, and the strict whole-file functions `readFile`,
`readFileLimited`, `readFileBytes`, `readFileBytesLimited`, `writeFile`,
`writeFileBytes`, and `appendFile`.

The unrestricted readers allocate the complete input. Prefer
`readFileLimited path maximumBytes` or
`readFileBytesLimited path maximumBytes` when input size is not already
trusted; they fail with `HostError ResourceExhausted` after reading at most one
byte beyond the bound. Text limits count encoded UTF-8 bytes. Use handles for genuinely
streaming input.

```solveig
copyText : String -> String -> Task ()
copyText source destination =
    withFile source ReadMode (fun input => do
        contents <- hGetContents input
        writeFile destination contents)
```

The garbage collector does not close handles. Prefer `withFile`; it uses
`std.task.bracket` so cleanup runs after success, typed error, defect, or
interruption.

Application filesystem work belongs in the `Path`-typed facade:

```solveig
import std.fs(*)
import std.path(*)
```

`std.path` provides `joinPath` and `(</>)`, `parent`, `fileName`, `extension`,
lexical `normalise`, and absolute/relative queries without touching the
filesystem. `std.fs` provides convenient strict and bounded text/byte reads,
ordinary and atomic writes, append operations, and scoped byte/text/line
streams in addition to `getCurrentDirectory`, `setCurrentDirectory`,
`doesFileExist`, `doesDirectoryExist`, `listDirectory`, `createDirectory`,
`removeFile`, `renameFile`, `getFileSize`, and `getModificationTime`. Its
actions return `IO FsError a`, retaining path context while keeping invalid
UTF-8 distinct from the underlying host cause. They accept opaque `Path`
values constructed with `pathFromString`;
`getModificationTime` returns `SystemTime`, not a bare integer.

The unrestricted strict readers are the concise choice for trusted files. Use
the `*Limited` forms for untrusted finite input and scoped streams for data that
may not fit memory. Atomic writes sync a sibling temporary file and then use a
single host rename; unsupported replacement fails without exposing a partial
new destination.

## Environment and processes

```solveig
import std.env(*)
import std.process(*)
```

`std.env` provides `getArgs`, `getEnv`, `getEnvOr`, `requireEnv`, `setEnv`,
`getEnvironment`, and `getProgName`. `requireEnv` uses the typed `EnvError`
rather than turning a missing setting into a defect.

`loadEnvFile` reads at most 64 KiB; `loadEnvFileLimited` accepts an explicit
bound; and `loadEnvFileIfExists` reports whether the file was present. Existing
process variables win over file assignments. The strict dotenv grammar accepts
blank lines, full-line comments, `NAME=VALUE`, optional `export `, and matching
single or double quotes. It deliberately performs no interpolation or shell
evaluation. Invalid errors retain a line number and reason, never the source
line that may contain a secret.

`std.process` owns validated opaque `Process` and `ExitCode` values,
binary-safe `ProcessResult`, its byte and explicit UTF-8 accessors,
request transformations for cwd (including restoring inherited cwd),
environment, stdin, and capture/discard,
`runProcess`, `runProcessLimited`, `runProcessTimeout`, and
the scoped `withProcess`/`ProcessHandle` API with pull-based stdout/stderr,
bounded backpressured stdin, waiting, and termination, plus
`exitWith`/`exitSuccess`/`exitFailure`.

`process program arguments` validates a request, which `runProcess` executes
without invoking a shell. A non-zero child status is `ProcessResult` data,
available through an opaque `ExitCode`. Failure to start or
capture the child is `ProcessError`. The unrestricted form allocates each
selected complete output stream as `Bytes`; decoding returns `Either
Utf8Error String`. `runProcessLimited processValue maximumPerStream`
bounds each stream in bytes; overflow terminates and reaps
the child and raises `ProcessError`. Discarded streams are drained without
retention. Timeout and interruption both terminate and reap the direct child.
For large or interactive children, `withProcess chunkSize request callback`
keeps the handle and its `Stream` values inside a rank-2 scope. Captured stdout
and stderr are independently pullable and should be consumed with
`concurrently` when both may fill; discarded streams drain automatically.
`pipeStandardInput` enables acknowledged chunks no larger than `chunkSize`,
and `closeProcessStdin` sends EOF. Every scope exit closes pipes and reaps the
direct child.

## Channels, threads, and structured tasks

```solveig
import std.channel(*)
import std.thread(*)
import std.task(*)
import std.time(Duration, sleep)
```

`std.channel` provides `newChan`, `newBoundedChan`, `send`, `recv`, `tryRecv`,
and `closeChan`. An invalid bounded capacity is a typed
`HostError InvalidArgument` rather than a defect.
`std.thread` exposes raw `spawn` and `killThread`.

Prefer `std.task` when lifetime or results matter:

```solveig
catchDefect : IO e a -> (Defect -> IO e a) -> IO e a
finally : IO e a -> IO e b -> IO e a
bracket : IO e resource -> (resource -> IO e cleanup) -> (resource -> IO e a) -> IO e a
mask : IO e a -> IO e a
withThread : IO e a -> (ThreadId -> IO e b) -> IO e b
concurrently : IO e a -> IO e b -> IO e (a, b)
race : IO e a -> IO e b -> IO e (Either a b)
timeout : Duration -> IO e a -> IO e (Maybe a)
mapConcurrentlyBoundedOrdered : Int -> (a -> IO e b) -> [a] -> IO (ConcurrentError e) [b]
mapConcurrentlyBoundedCompletionOrder : Int -> (a -> IO e b) -> [a] -> IO (ConcurrentError e) [b]
```

Both bounded maps scope and join their worker group, cap active operations, and
preserve worker errors separately from pool infrastructure. The ordered form
returns input order; the completion-order form retains arrival order. A bounded
job channel applies worker-count backpressure. Use these functions for
independent IO operations; keep sequential `mapM` for small CPU-only
transformations where scheduling would add overhead.

## Testing helpers

`std.testing` provides pure assertions, IO/resource assertions, and explicitly
seeded property generators. It deliberately does not introduce another test
runner, global registry, discovery convention, or exception path.
`TestFailure` has `Eq` and `Show`; use a program's `main` to render the result:

```solveig
checks : Either std.testing.TestFailure ()
checks = std.testing.checkAll
    [ std.testing.assert "queue is non-empty" (not (std.queue.isEmpty queue))
    , std.testing.assertEqual "queue length" 2 (std.queue.length queue)
    ]
```

`Gen a` is opaque and pure. `generate` returns both the value and next
`StdGen`; `chooseInt`, `listOf`, `mapGen`, `pairGen`, and `constantGen` compose
without ambient generator state. `checkProperty` requires a positive exact case
count and caller-provided `StdGen`; success returns the next generator state,
while failure records the one-based case and displayed value. Reusing the same
seed, generator, and case count reproduces the same run.

```solveig
case std.testing.chooseInt (-100) 100 of
    Nothing => error "invalid generator bounds"
    | Just integers =>
        std.testing.checkProperty
            "doubling is even"
            100
            integers
            (even . (2 *))
            (mkStdGen 42)
```

`assertIO` preserves the checked action's typed error. `assertWithResource`
specializes `std.task.bracket`: it acquires once, evaluates an IO Boolean, and
runs release after success, assertion failure, typed failure, defect, or
interruption. It returns an ordinary `Either TestFailure ()` inside the same
`IO e`; it does not catch or reclassify resource errors.

## Time and randomness

```solveig
import std.time(*)
import std.random(*)
```

`sleep` and `timeout` accept `Duration`, constructed with `nanoseconds`,
`microseconds`, `milliseconds`, or `seconds`. `getMonotonicTime` returns
`Instant`; `getSystemTime` returns the distinct wall-clock `SystemTime`.
`measure task` returns `(elapsedDuration, value)`. Unit-bearing conversions
such as `durationToMilliseconds` and `durationToSeconds` are explicit;
construction and clock differences saturate rather than overflowing.

Random draws use explicit immutable `StdGen` state. `mkStdGen` is reproducible;
`newStdGen` obtains a runtime-local clock seed. `randomInt` returns
`(value, nextGenerator)`; inclusive `randomRange` returns `Nothing` for
reversed bounds and `Just (value, nextGenerator)` otherwise. This
generator is not cryptographically secure. `secureRandomBytes count` is the
separate cryptographic effect: it returns `IO SecureRandomError Bytes`, obtains
host entropy on the bounded worker pool, and is intentionally neither seeded
nor reproducible. Cancellation stops the caller from waiting but cannot cancel
an operating-system entropy call already in progress.

## Networking

```solveig
import std.bytes(*)
import std.net(*)
```

`std.net` provides `connect`, `resolveHost`, `listen`, `accept`, `sendBytes`,
`recvBytes`, `closeSocket`, and `closeListener`. Prefer `withConnection`,
`withListener`, and `withAcceptedConnection`; their bracketed scopes close
owned sockets after success, typed errors, defects, or interruption. These
actions use `NetError`, retaining endpoint context and the host cause.
Network operations accept validated `Port` values; `portFromInt` checks an
external integer and `anyPort` explicitly requests an ephemeral listener.

The low-level `std.net` layer is TCP-oriented and byte-based. Use `std.http`
for bounded HTTP/1.1 and verified HTTPS rather than hand-building wire frames.
UDP and general IPv6 selection are not provided. Scoped streaming subprocess
pipes are provided by `std.process`.

### HTTP and HTTPS

```solveig
import std.http(*)
import std.time

fetchRelease : String -> IO std.http.HttpError std.http.Response
fetchRelease repository =
    std.http.getWith
        (std.http.withHeader "Accept" "application/json" $
            std.http.withRedirectPolicy std.http.NoRedirects $
                std.http.withTimeout (std.time.seconds 5) $
                    std.http.withMaximumBufferedBodyBytes 1048576 std.http.defaultOptions)
        ("https://api.example.test/repos/" <> repository <> "/release")
```

Use `get url` for the common bounded request. `post`, `put`, `patch`, `delete`,
`head`, and `options` have the same direct shape. Their `With` variants accept
immutable `HttpOptions` built from `defaultOptions`; `withHeader`, `withQuery`,
and the body setters cover ordinary API clients without exposing wire framing.
`request method url` handles a dynamically chosen method.

Every buffered request applies practical response-header, response-body,
timeout, and redirect bounds. Statuses such as 404 remain ordinary `Response` values;
transport, certificate/TLS, timeout, redirect, and size failures are typed.
HTTPS verifies certificates through the host platform policy. Strict response bodies
are fully bounded before entering the Solveig heap. Use `bodyChunks` when a
list of bounded pieces is convenient, or `foldBodyChunks` to process those
pieces without allocating that list.

`withResponse method url callback` instead provides status, headers, and a rank-2
scoped `Stream scope HttpError Bytes` before the body is consumed. Default pulls
are at most 32 KiB; `withResponseWith options method url callback` can add a
total quota and change the pull bound. `requestStream` and `withStreamResponse`
send a demand-driven one-shot byte stream. Builder bodies retain chunks and
file bodies upload incrementally. Early return, producer failure, defects, and
cancellation close resources; streaming upload owner threads are always joined.

Header validation rejects invalid names, newline injection, and attempts to set
client-managed framing headers. Query and form encoding uses the host URL
library. Header lookup is ASCII case-insensitive. The
[networking tutorial](tutorial/07-networking-and-http.md)
demonstrates typed failures and bounded concurrent requests against a local
service. See [HTTP and HTTPS](http.md) for the complete client policy.
