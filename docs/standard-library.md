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
| `std.text` | Text/character conversion and integer rendering |
| `std.bytes` | UTF-8 conversion and immutable byte operations |
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
import std.list(take, drop, zip, reverse, range)
import std.text(stringToChars, charsToString, intToString)
```

`std.list` provides `take`, `drop`, `takeWhile`, `dropWhile`, `zip`,
`zipWith`, `reverse`, and inclusive-or-exclusive integer `range`. These are
separate from the universally imported traversal operations such as `map` and
`foldl`.

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

## Handles and files

```solveig
import std.io
```

`std.io` owns `IOMode`, `BufferMode`, the standard handles, incremental handle
operations, `withFile`, and the strict whole-file functions `readFile`,
`readFileBytes`, `writeFile`, `writeFileBytes`, and `appendFile`.

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
`runProcess`, and `exitWith`/`exitSuccess`/`exitFailure`.

`runProcess` receives an executable and an argument list; it never invokes a
shell. A non-zero child status is `ProcessResult` data. Failure to start or
capture the child is `RunProcessError`.

## Channels, threads, and structured tasks

```solveig
import std.channel
import std.thread
import std.task
import std.time(sleep)
```

`std.channel` provides `newChan`, `newBoundedChan`, `send`, `recv`, `tryRecv`,
and `closeChan`. `std.thread` exposes raw `spawn` and `killThread`.

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
`newStdGen` obtains a runtime-local clock seed. `randomInt` and inclusive
`randomRange` return `Pair value nextGenerator`. This generator is not
cryptographically secure.

## Networking

```solveig
import std.bytes
import std.net
```

`std.net` provides `connect`, `resolveHost`, `listen`, `accept`, `sendBytes`,
`recvBytes`, and `closeSocket`. Sockets are explicit resources; close them with
`std.task.bracket` or another cleanup scope.

The current network layer is TCP-oriented and byte-based. HTTP, URL parsing,
structured codecs, and streaming subprocess pipes remain future library work.
