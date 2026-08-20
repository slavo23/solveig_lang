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
| `std.list` | Extended list slicing, zipping, reversal, and ranges |
| `std.vector` | Persistent indexed sequences with logarithmic updates |
| `std.map` | Persistent ordered key-value maps |
| `std.set` | Persistent ordered sets |
| `std.queue` | Persistent FIFO queues |
| `std.text` | Text/character conversion and integer rendering |
| `std.bytes` | UTF-8 conversion and immutable byte operations |
| `std.parser` | Generic parser combinators with structured errors and streaming input |
| `std.json` | Bounded byte-level JSON, exact numbers, deterministic rendering, and direct decoders |
| `std.http` | Bounded HTTP/HTTPS requests, verified TLS, redirects, status/header access, and chunk folds |
| `std.io` | Handles, buffering, and whole-file IO |
| `std.fs` | Directory, path, and file-metadata operations |
| `std.env` | Arguments, environment, and program identity |
| `std.process` | Child processes and structured program exit |
| `std.channel` | Typed channel creation and communication |
| `std.thread` | Raw lightweight-thread creation and interruption |
| `std.task` | Cleanup, masking, and structured concurrency |
| `std.time` | Sleeping and clock readings |
| `std.random` | Explicit deterministic pseudorandom state |
| `std.net` | TCP sockets and hostname resolution |

## Lists and text

```solveig
import std.list(take, drop, zip, reverse, range, rangeInclusive)
import std.text(stringToChars, charsToString, intToString)
```

`std.list` provides `take`, `drop`, `takeWhile`, `dropWhile`, `zip`,
`zipWith`, `reverse`, half-open integer `range`, and explicit
`rangeInclusive`. These are
separate from the universally imported traversal operations such as `map` and
`foldl`. `take` treats a non-positive count as empty; `drop` treats it as zero.

## Persistent collections

```solveig
import std.vector
import std.map
import std.set
import std.queue
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

This is an immutable functional vector, not a contiguous mutable array. The
tree representation is deliberate: it provides predictable persistent updates
using language-level algebraic data today. A future native wide-branch vector
may lower the constant factor without changing the safe API.

`Map` and `Set` use persistent AVL trees and therefore require `Ord` keys or
elements. Lookup, insertion, and deletion are logarithmic; `size` is constant
time; ordered traversal is linear and deterministic. `Map.fromList` keeps the
last value for a duplicate key. `Set.fromList` discards duplicates.

```solveig
phoneBook : std.map.Map String String
phoneBook =
    std.map.fromList
        [ std.map.Entry "Ada" "+44"
        , std.map.Entry "Linus" "+358"
        ]

names : [String]
names = std.map.keys phoneBook
```

`Queue` uses a front list and a reversed rear list. `enqueue` and `dequeue`
are amortized constant time, `length` is constant time, and `toList` is linear.
`dequeue` returns both the oldest value and the persistent remainder:

```solveig
next : Maybe (Pair String (std.queue.Queue String))
next =
    std.queue.dequeue
        (std.queue.enqueue (std.queue.enqueue std.queue.empty "first") "second")
```

The exported collection constructors describe the current representation and
should not be used by application code; use the smart constructors and
operations above so cached balance and size invariants remain valid.

## Bytes

```solveig
import std.bytes
```

```solveig
stringToBytes : String -> Bytes
bytesToString : Bytes -> Result IOError String
bytesLength : Bytes -> Int
bytesIndex : Bytes -> Int -> Maybe Int
bytesSlice : Bytes -> Int -> Int -> Bytes
bytesAppend : Bytes -> Bytes -> Bytes
```

`String` is valid UTF-8; `Bytes` preserves arbitrary data. Decoding therefore
returns `Result` instead of replacing invalid input.

The bulk `bytesSpanAny`, `bytesFindAny`, `bytesFindBelow`, and
`bytesSpanRange` operations scan without constructing a `Maybe` for each byte.
`bytesSliceToString` validates one slice without first allocating another
`Bytes` value. These operations are useful when implementing byte-oriented
formats; ordinary callers should prefer the higher-level parser for that
format.

## Parser combinators

```solveig
import std.parser
import std.text
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
apply. In an editor,
hover the operation you are using; the method documentation is intended to be
the primary learning path. The complete configuration grammar in
`tests/golden/run/parser_config` demonstrates nesting, custom errors,
deliberate backtracking, and parsing across inconvenient chunk boundaries.

## JSON

```solveig
import std.bytes
import std.json
import std.parser
import std.vector

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
Rendering revalidates even a directly constructed `JsonNumber`, preserving a
clean future path to a `BigInt` representation.

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
`std.json.renderJsonError` through `std.parser.renderError`.

## Handles and files

```solveig
import std.io
```

`std.io` owns `IOMode`, `BufferMode`, the standard handles, incremental handle
operations, `withFile`, and the strict whole-file functions `readFile`,
`readFileLimited`, `readFileBytes`, `readFileBytesLimited`, `writeFile`,
`writeFileBytes`, and `appendFile`.

The unrestricted readers allocate the complete input. Prefer
`readFileLimited path maximumBytes` or
`readFileBytesLimited path maximumBytes` when input size is not already
trusted; they fail with `ReadFileError` after reading at most one byte beyond
the bound. Text limits count encoded UTF-8 bytes. Use handles for genuinely
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
import std.fs
```

This module provides `getCurrentDirectory`, `setCurrentDirectory`,
`doesFileExist`, `doesDirectoryExist`, `listDirectory`, `createDirectory`,
`removeFile`, `renameFile`, `getFileSize`, and `getModificationTime`.

## Environment and processes

```solveig
import std.env
import std.process
```

`std.env` provides `getArgs`, `getEnv`, `setEnv`, `getEnvironment`, and
`getProgName`. `std.process` owns `ProcessResult`, its three accessors,
`runProcess`, `runProcessLimited`, and
`exitWith`/`exitSuccess`/`exitFailure`.

`runProcess` receives an executable and an argument list; it never invokes a
shell. A non-zero child status is `ProcessResult` data. Failure to start or
capture the child is `RunProcessError`. The unrestricted form allocates both
complete output streams. `runProcessLimited program arguments
maximumPerStream` bounds each stream in bytes; overflow terminates and reaps
the child and raises `RunProcessError`. Interrupting the calling thread also
terminates and reaps its direct child.

## Channels, threads, and structured tasks

```solveig
import std.channel
import std.thread
import std.task
import std.time(sleep)
```

`std.channel` provides `newChan`, `newBoundedChan`, `send`, `recv`, `tryRecv`,
and `closeChan`. An invalid bounded capacity is the typed
`InvalidChannelCapacity capacity` error rather than a defect.
`std.thread` exposes raw `spawn` and `killThread`.

Prefer `std.task` when lifetime or results matter:

```solveig
catchDefect : IO e a -> (Defect -> IO e a) -> IO e a
finally : IO e a -> IO e b -> IO e a
bracket : IO e resource -> (resource -> IO e cleanup) -> (resource -> IO e a) -> IO e a
mask : IO e a -> IO e a
withThread : IO e a -> (ThreadId -> IO e b) -> IO e b
concurrently : IO e a -> IO e b -> IO e (Pair a b)
race : IO e a -> IO e b -> IO e (Either a b)
timeout : Int -> IO e a -> IO e (Maybe a)
```

## Time and randomness

```solveig
import std.time
import std.random
```

`sleep` and `timeout` use milliseconds. `getMonotonicTime` and
`getSystemTime` return nanoseconds. Use monotonic differences for durations.

Random draws use explicit immutable `StdGen` state. `mkStdGen` is reproducible;
`newStdGen` obtains a runtime-local clock seed. `randomInt` returns
`Pair value nextGenerator`; inclusive `randomRange` returns `Nothing` for
reversed bounds and `Just (MkPair value nextGenerator)` otherwise. This
generator is not cryptographically secure.

## Networking

```solveig
import std.bytes
import std.net
```

`std.net` provides `connect`, `resolveHost`, `listen`, `accept`, `sendBytes`,
`recvBytes`, `closeSocket`, and `closeListener`. Prefer `withConnection`,
`withListener`, and `withAcceptedConnection`; their bracketed scopes close
owned sockets after success, typed errors, defects, or interruption.

The low-level `std.net` layer is TCP-oriented and byte-based. Use `std.http`
for bounded HTTP/1.1 and verified HTTPS rather than hand-building wire frames.
Streaming subprocess pipes, UDP, and general IPv6 selection remain future
library work.

### HTTP and HTTPS

```solveig
import std.http

fetch : String -> IO std.http.HttpError std.http.Response
fetch spelling =
    case std.http.urlFromString spelling of
        Err error => failIO error
        | Ok url => std.http.request std.http.defaultHttpLimits (std.http.get url)
```

`std.http.request` always applies response-header, response-body, timeout, and
redirect bounds. Statuses such as 404 remain ordinary `Response` values;
transport, certificate/TLS, timeout, redirect, and size failures are typed.
HTTPS verifies certificates through the host platform policy. Response bodies
are fully bounded before entering the Solveig heap. Use `bodyChunks` when a
list of bounded pieces is convenient, or `foldBodyChunks` to process those
pieces without allocating that list.

Header constructors reject invalid names and newline injection. Header lookup
is ASCII case-insensitive. `examples/release_radar` demonstrates concurrent
requests, JSON decoding, stable result ordering, ETag request headers, and a
single overwritten cache file. See [HTTP and HTTPS](http.md) for policies and
complete examples.
