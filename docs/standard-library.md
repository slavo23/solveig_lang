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
| `std.json` | Bounded byte-level JSON, exact numbers, deterministic rendering, and direct decoders |
| `std.http` | Convenient bounded GETs plus explicit HTTP/HTTPS requests, verified TLS, statuses, headers, and chunk folds |
| `std.io` | Handles, buffering, and whole-file IO |
| `std.fs` | Directory, path, and file-metadata operations |
| `std.env` | Arguments, environment lookup, bounded dotenv loading, and program identity |
| `std.process` | Child processes and structured program exit |
| `std.channel` | Typed channel creation and communication |
| `std.thread` | Raw lightweight-thread creation and interruption |
| `std.task` | Cleanup, masking, structured concurrency, and bounded concurrent mapping |
| `std.testing` | Small pure assertion values for checks in ordinary programs |
| `std.time` | Sleeping, clock readings, and monotonic measurement |
| `std.random` | Explicit deterministic pseudorandom state |
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
`between`; then use `runComplete` when trailing input must be rejected. Choice
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

Use `parseJson` when the whole `Json` tree is useful. Use `decodeJson` with
`stringDecoder`, `boolDecoder`, `numberDecoder`, `nullDecoder`,
`arrayDecoder`, `objectDecoder`, or `nullableDecoder` when the requested value
can be built directly. For example, this creates a vector of strings without
first allocating a vector of `JsonString` nodes:

```solveig
names : std.parser.ParseReply std.parser.ByteInput std.json.JsonError
    (std.vector.Vector String)
names =
    std.json.decodeJson std.json.defaultJsonLimits
        (std.json.arrayDecoder std.json.stringDecoder)
        (std.bytes.stringToBytes "[\"Ada\",\"Grace\"]")
```

Both entry points require complete input apart from JSON whitespace and return
the same structured `std.parser.ParseReply`. Render custom failures with
`std.json.renderJsonError` through `std.parser.renderError`. When the custom
error itself has `Show`, `ParseError custom` does too, so `show error` is the
concise equivalent.

## Handles and files

```solveig
import std.io(*)
```

`std.io` owns `IOMode`, `BufferMode`, the standard handles, incremental handle
operations, `withFile`, and the strict whole-file functions `readFile`,
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

Filesystem metadata and directory operations are separate:

```solveig
import std.fs(*)
import std.path(*)
```

This module provides `getCurrentDirectory`, `setCurrentDirectory`,
`doesFileExist`, `doesDirectoryExist`, `listDirectory`, `createDirectory`,
`removeFile`, `renameFile`, `getFileSize`, and `getModificationTime`. Its
actions return `IO FsError a`, retaining path context and the underlying host
cause. They accept opaque `Path` values constructed with `pathFromString`;
`getModificationTime` returns `SystemTime`, not a bare integer.

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
`ProcessResult`, its three accessors,
`runProcess`, `runProcessLimited`, and
`exitWith`/`exitSuccess`/`exitFailure`.

`process program arguments` validates a request, which `runProcess` executes
without invoking a shell. A non-zero child status is `ProcessResult` data,
available through an opaque `ExitCode`. Failure to start or
capture the child is `ProcessError`. The unrestricted form allocates both
complete output streams. `runProcessLimited processValue maximumPerStream`
bounds each stream in bytes; overflow terminates and reaps
the child and raises `ProcessError`. Interrupting the calling thread also
terminates and reaps its direct child.

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
mapConcurrentlyBounded : Int -> (a -> IO e b) -> [a] -> IO (ConcurrentError e) [b]
```

`mapConcurrentlyBounded` scopes and joins its worker group, caps in-flight
operations, preserves worker errors separately from pool infrastructure, and
returns results in input order even when completion order differs. Use it for
independent IO operations; keep sequential `mapM` for
small CPU-only transformations where scheduling would add overhead.

## Testing helpers

`std.testing` provides pure `assert`, `assertEqual`, `checkAll`, and
`failureMessage` operations returning `Either TestFailure`. It deliberately
does not introduce another test runner, global registry, or exception path.
`TestFailure` has `Eq` and `Show`; use a program's `main` to render the result:

```solveig
checks : Either std.testing.TestFailure ()
checks = std.testing.checkAll
    [ std.testing.assert "queue is non-empty" (not (std.queue.isEmpty queue))
    , std.testing.assertEqual "queue length" 2 (std.queue.length queue)
    ]
```

## Time and randomness

```solveig
import std.time(*)
import std.random(*)
```

`sleep` and `timeout` accept `Duration`, constructed with `nanoseconds`,
`microseconds`, `milliseconds`, or `seconds`. `getMonotonicTime` returns
`Instant`; `getSystemTime` returns the distinct wall-clock `SystemTime`.
`measure task` returns `(elapsedDuration, value)`.

Random draws use explicit immutable `StdGen` state. `mkStdGen` is reproducible;
`newStdGen` obtains a runtime-local clock seed. `randomInt` returns
`(value, nextGenerator)`; inclusive `randomRange` returns `Nothing` for
reversed bounds and `Just (value, nextGenerator)` otherwise. This
generator is not cryptographically secure.

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
Streaming subprocess pipes, UDP, and general IPv6 selection are not provided.

### HTTP and HTTPS

```solveig
import std.http(*)
import std.time

fetchRelease : String -> IO std.http.HttpError std.http.Response
fetchRelease repository =
    std.http.fetchWith
        (std.http.withRedirectPolicy
            std.http.NoRedirects
            (std.http.withHttpTimeout
                (std.time.milliseconds 5000)
                (std.http.withMaximumBodyBytes
                    1048576
                    (std.http.withMaximumHeaderBytes
                        65536
                        std.http.defaultHttpLimits))))
        [("Accept", "application/json")]
        ("https://api.example.test/repos/" <> repository <> "/release")
```

Use `fetch spelling` for a bounded GET with practical defaults, and
`fetchWith limits headers spelling` when policy or headers differ. Both
validate URL and header strings before performing IO. The lower-level
`Url`/`Header`/`Request` builders and `request` remain available for other
methods and bodies; `send` applies default limits to a prepared request.

Every request applies response-header, response-body, timeout, and redirect
bounds. Statuses such as 404 remain ordinary `Response` values;
transport, certificate/TLS, timeout, redirect, and size failures are typed.
HTTPS verifies certificates through the host platform policy. Response bodies
are fully bounded before entering the Solveig heap. Use `bodyChunks` when a
list of bounded pieces is convenient, or `foldBodyChunks` to process those
pieces without allocating that list.

Header validation rejects invalid names and newline injection. Header lookup
is ASCII case-insensitive. The [networking tutorial](tutorial/07-networking-and-http.md)
demonstrates typed failures and bounded concurrent fetches against a local
service. See [HTTP and HTTPS](http.md) for the complete client policy.
