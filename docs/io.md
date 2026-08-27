# Input and output

Solveig's `Task a` is `IO HostError a`: an inert description that the runtime
performs when it runs `main`. Blocking file and standard-input operations run
on the bounded IO worker pool, so a blocked handle does not block lightweight
threads or the scheduler. Open files are distributed across that pool and all
operations for one handle stay FIFO.

Host-facing APIs live in explicit standard-library modules rather than the
implicit Prelude. Import only the capabilities a module uses:

```solveig
import std.io       # handles and whole-file IO
import std.fs       # filesystem queries and mutations
import std.path     # validated paths
import std.env      # arguments and environment
import std.process  # child processes and exit
import std.time     # clocks
import std.random   # pseudorandom generators
import std.bytes    # byte conversion and manipulation
```

The Prelude still provides `Task`, typed-error sequencing, `printLine`,
`printStr`, and `readLine`, since they are useful in nearly every executable.

Incremental data uses the shared [`std.stream`](streaming.md) pull protocol.

## Handles and ownership

`Handle`, `IOMode`, and `BufferMode` are exported by `std.io`. A handle is only an opaque identity
into one runtime's host-resource table; a Solveig program cannot inspect or
construct one. Handles are not heap objects.

**GC never closes a handle.** Use `withFile` for normal code, or call `hClose`
explicitly when ownership must cross a larger scope. `withFile` is defined via
`bracket`, so it closes after success, a typed error, a defect, or thread
interruption:

```solveig
withFile : forall a. String -> IOMode -> (Handle -> Task a) -> Task a

copyText : String -> String -> Task ()
copyText source destination =
    withFile source ReadMode (fun input => do
        contents <- hGetContents input
        withFile destination WriteMode (fun output =>
            hPutStr output contents))
```

Using a closed handle raises `HostError ResourceVanished` with the attempted
operation recorded in `hostErrorOperation`. Handle identities
are not reused within a runtime, so a stale identity cannot alias a later file.

## Scoped file streams

Use `withFileByteStream` when the complete file should not be retained in
memory or the consumer may stop early:

```solveig
import std.bytes(bytesLength)
import std.io(withFileByteStream)
import std.stream(Stream, fold, take)

countPrefix : forall scope. Stream scope HostError Bytes -> Task Int
countPrefix stream =
    fold
        (fun total bytes => total + bytesLength bytes)
        0
        (take 4 stream)

main : Task ()
main = do
    byteCount <- withFileByteStream 8192 "input.bin" countPrefix
    printLine (show byteCount)
```

Each pull returns at most 8 KiB in this example and there is no read-ahead.
Returning after four chunks closes the file without draining it. The rank-2
callback prevents the stream from escaping, and cleanup runs after success,
typed failure, defect, or interruption. The chunk size bounds per-pull memory;
it is not a total input limit. See [Strict streams](streaming.md) for the
single-consumer protocol and combinator buffering guarantees.

## Opening and closing

```solveig
data IOMode = ReadMode | WriteMode | AppendMode | ReadWriteMode

openFile : String -> IOMode -> Task Handle
hClose : Handle -> Task ()
hFlush : Handle -> Task ()
```

`openFile` resolves relative paths against the logical working directory and
returns `HostError` with operation `openFile` on failure.

- `ReadMode` opens an existing readable file.
- `WriteMode` creates the file if necessary and truncates an existing file.
- `AppendMode` creates the file if necessary and writes at the end.
- `ReadWriteMode` opens an existing file for reading and writing without
  truncating or creating it.

`hClose` first flushes buffered output and closes the logical handle. Closing
an already closed handle returns `HostError ResourceVanished`. `hFlush`
writes pending output to the host stream; failures retain operation `hFlush`.
Normal runtime shutdown makes a
best-effort flush, but it cannot report failures to a finished program and is
not a substitute for `withFile` or `hClose`.

## Text input

```solveig
hGetLine : Handle -> Task String
hGetContents : Handle -> Task String
hIsEOF : Handle -> Task Bool
```

Text is decoded strictly as UTF-8. Invalid UTF-8, a handle opened without read
permission, or a host read failure returns `HostError` with the attempted
operation retained. `hGetLine`
removes `\n` and an immediately preceding `\r`; at EOF it returns `""`. Use
`hIsEOF` first when an empty final line must be distinguished from EOF.

`hGetContents` strictly reads and allocates all remaining text before the
action succeeds. This deliberately differs from Haskell's lazy
`hGetContents`: it does not defer reads, keep a hidden semi-closed handle, or
move failures outside the action. For large or binary inputs, use `hGetBytes`
so decoding remains explicit.

`hIsEOF` may fill the handle's read buffer, but it does not consume a byte.
Read-ahead is internal and preserves the program-visible position.

## Text output

```solveig
hPutStr : Handle -> String -> Task ()
hPutStrLn : Handle -> String -> Task ()
```

Both operations encode `String` as UTF-8. `hPutStrLn` appends `\n`; it does not
translate it to the host platform's native text newline. A handle opened
without write permission, a closed handle, or a host write failure returns
`HostError` with its portable kind and operation. Success means the text reached the selected
language buffer; use `hFlush` when the host must observe it immediately.

## Buffering

```solveig
data BufferMode =
    NoBuffering
  | LineBuffering
  | BlockBuffering (Maybe Int)

hSetBuffering : Handle -> BufferMode -> Task ()
```

`NoBuffering` writes each operation immediately. `LineBuffering` flushes after
an operation that writes a newline. `BlockBuffering Nothing` uses an 8 KiB
default; `BlockBuffering (Just size)` uses a positive byte size. A non-positive
or host-unrepresentable size returns `HostError InvalidArgument`.

Changing buffering first flushes pending writes and preserves the logical read
position. A failure retains operation `hSetBuffering`. Terminal `stdout` and
`stderr` default to line
buffering; redirected/captured streams and files default to block buffering.
`stdin` is line-oriented. The embedding API supplies `BufRead`/`Write` streams,
so host-side buffering below Solveig may add another layer.

The compatibility wrappers are handle-based:

```solveig
stdin : Handle
stdout : Handle
stderr : Handle
readLine : Task String
printLine : String -> Task ()
printStr : String -> Task ()
```

`readLine` is `hGetLine stdin` and retains the old `""` result at EOF.
`printLine` uses `hPutStrLn stdout`. `printStr` uses
`hPutStr stdout` and then flushes, preserving its practical prompt-writing
behavior regardless of redirection. Direct `hPutStr` calls retain full control
over buffering. Failures use `HostError`; its operation distinguishes reads,
writes, and flushes, including when a standard handle was explicitly closed.

## Whole-file text input

```solveig
readFile : String -> Task String
readFileLimited : String -> Int -> Task String
```

Both operations strictly decode UTF-8 before succeeding. `readFile` has no
library size bound and should be used only when the complete input comfortably
fits memory. `readFileLimited path maximum` counts encoded bytes, reads at most
`maximum + 1`, and raises `HostError ResourceExhausted` when the input is larger
or `HostError InvalidArgument` when the maximum is invalid. This is a runtime-enforced bound, not a metadata
pre-check that can race with file growth.

For application code, prefer the `Path`-typed `std.fs` facade. It keeps path
context on every failure and distinguishes malformed text from an operating
system failure:

```solveig
data FsError =
    FsHostError Path HostError
  | FsInvalidUtf8 Path Utf8Error

readFile : Path -> IO FsError String
readFileLimited : Path -> Int -> IO FsError String
readFileBytes : Path -> IO FsError Bytes
readFileBytesLimited : Path -> Int -> IO FsError Bytes
writeFile : Path -> String -> IO FsError ()
writeFileAtomic : Path -> String -> IO FsError ()
writeFileBytes : Path -> Bytes -> IO FsError ()
writeFileBytesAtomic : Path -> Bytes -> IO FsError ()
appendFile : Path -> String -> IO FsError ()
appendFileBytes : Path -> Bytes -> IO FsError ()
```

The unrestricted readers are deliberately convenient for ordinary trusted
files; callers do not have to invent an arbitrary limit. Use the bounded forms
at an untrusted-input boundary and scoped streams when input may not fit in
memory. `writeFileAtomic` and `writeFileBytesAtomic` write and sync a sibling
temporary file before replacing the destination with one host rename. A host
that cannot atomically replace an existing destination returns `FsHostError`;
the old destination remains intact and the temporary file is removed.

## Byte input and output

```solveig
readFileBytes : String -> Task Bytes
readFileBytesLimited : String -> Int -> Task Bytes
writeFileBytes : String -> Bytes -> Task ()
hGetBytes : Handle -> Int -> Task Bytes
hPutBytes : Handle -> Bytes -> Task ()
```

These operations transfer arbitrary bytes without encoding or newline
translation. `readFileBytes` strictly reads and allocates the entire file with
no library size bound. `readFileBytesLimited path maximum` reads at most the
non-negative byte limit and raises `HostError ResourceExhausted` if the file has
even one more byte; the runtime reads no more than `maximum + 1` bytes to make
that decision, so it does not rely on racy file metadata. A negative or
host-unrepresentable maximum also raises `HostError InvalidArgument`.

`writeFileBytes` creates or replaces a file just like `writeFile`, but writes
the supplied bytes unchanged. They bypass handle buffering and raise a
`HostError` carrying `readFileBytes` or `writeFileBytes` as its operation.

`hGetBytes handle maximum` reads at most the non-negative `maximum`. It returns
fewer bytes at EOF and returns empty bytes when called at EOF or with zero.
Regular files fill the requested count unless EOF is reached; `stdin` may
return a shorter chunk when that is what the embedding host stream makes
available. A negative or host-unrepresentable maximum, a non-readable or
closed handle, and host failures raise `HostError` with operation `hGetBytes`.

`hPutBytes` uses precisely the same buffer as text output. Under line
buffering, a byte value `10` triggers a flush; block-buffer sizes count bytes.
Success means the bytes reached the language buffer, so use `hFlush` when the
host must observe them immediately. A non-writable or closed handle and host
failures raise `HostError` with operation `hPutBytes`.

Text and byte operations can be mixed on one handle without changing its
logical position. Text input still decodes strictly: invalid UTF-8 raises the
operation's read error. Byte input never decodes. Convert explicitly with
`bytesToString`; it returns `Left (InvalidUtf8 byteOffset)` rather than panicking or
replacing malformed input. Use the conspicuously named
`bytesToStringReplacing` only when substituting U+FFFD is the intended policy.

## Environment and program identity

```solveig
getEnv : String -> Task (Maybe String)
setEnv : String -> String -> Task ()
getEnvironment : Task [(String, String)]
getProgName : Task String
```

The runtime snapshots UTF-8 host environment entries at startup. `setEnv`
changes this runtime-local copy, and subsequent environment reads observe the
change; it does not mutate the host process's global environment. Names must be
non-empty and contain neither `=` nor NUL,
and values cannot contain NUL. Invalid inputs return `HostError
InvalidArgument` with the environment operation retained. `getEnvironment`
is sorted by variable name for deterministic
iteration.

`getProgName` currently returns the project root's final directory component.
Non-UTF-8 host entries are omitted
because Solveig `String` is UTF-8.

## Clocks

```solveig
getMonotonicTime : IO e Instant
getSystemTime : IO e SystemTime
```

The values are opaque and cannot be mixed accidentally. `getMonotonicTime`
measures elapsed time
from an arbitrary origin local to one runtime and never decreases; subtract
two readings to measure a duration. Its absolute value has no relationship to
a calendar clock. Use `instantDifference` to produce `Duration`.
`getSystemTime` is a signed count since the Unix epoch and
can move when the host clock is adjusted. Values outside the `Int` nanosecond
range saturate instead of wrapping.

The operations execute directly on the scheduler thread: neither blocks nor
uses an IO worker. Normal execution reads the host clocks. Captured execution
and `SOLVEIG_SCHED_DETERMINISTIC=1` expose the scheduler's logical elapsed time
for both operations, with the virtual system clock beginning at the Unix
epoch. A virtual `sleep (milliseconds 5)` therefore advances both readings by exactly
5,000,000 nanoseconds. This makes clock-dependent program tests exact while
keeping the same action and scheduler paths as normal execution.

## Pseudorandom generation

```solveig
mkStdGen : Int -> StdGen
newStdGen : Task StdGen
randomInt : StdGen -> (Int, StdGen)
randomRange : Int -> Int -> StdGen -> Maybe (Int, StdGen)
```

`StdGen` is opaque, immutable SplitMix64 state. `mkStdGen` makes a repeatable
generator from any `Int`; the sequence for a fixed seed is part of Solveig's
behavior. Each pure draw returns both its result and the next generator. Reuse
the same input generator to replay the same result, or thread the returned
state through later draws:

```solveig
case randomRange 1 6 (mkStdGen 42) of
    Nothing => Nothing
    | Just (die, next) => randomRange 1 6 next
```

`randomInt` covers the complete signed `Int` range. `randomRange lower upper`
uses inclusive bounds and unbiased rejection sampling. It returns `Nothing`
when the bounds are reversed and `Just (value, nextGenerator)` otherwise.
Because rejection may consume another word, callers must treat the returned
generator—not a guessed step count—as the next state.

Only `newStdGen` performs IO. It draws a distinct generator seed from one
runtime-local SplitMix stream initially seeded from the runtime's system clock.
Captured and deterministic execution begin that stream at seed zero, so tests
remain exact. Clock-derived seeds can repeat across runtimes and are not host
entropy. These APIs are pseudorandom and must not be used for keys, tokens,
nonces, passwords, or any other cryptographic purpose.

## Filesystem and logical working directory

`std.path.Path` is opaque rather than a synonym for `String`. Its pure path
algebra follows the host platform without accessing the filesystem:

```solveig
pathFromString : String -> Either PathError Path
pathToString : Path -> String
joinPath : Path -> Path -> Path
(</>) : Path -> Path -> Path
parent : Path -> Maybe Path
fileName : Path -> Maybe String
extension : Path -> Maybe String
normalise : Path -> Path
isAbsolute : Path -> Bool
isRelative : Path -> Bool
```

`normalise` is lexical: it removes redundant `.` components and collapses
ordinary `name/..` pairs, but it does not resolve symbolic links, canonicalise
case, or require a path to exist. An absolute right operand of `(</>)` replaces
the left operand according to the host's path rules. `extension` omits the
leading dot.

```solveig
getCurrentDirectory : IO FsError Path
setCurrentDirectory : Path -> IO FsError ()
doesFileExist : Path -> IO FsError Bool
doesDirectoryExist : Path -> IO FsError Bool
listDirectory : Path -> IO FsError [Path]
createDirectory : Path -> IO FsError ()
removeFile : Path -> IO FsError ()
renameFile : Path -> Path -> IO FsError ()
getFileSize : Path -> IO FsError Int
getModificationTime : Path -> IO FsError SystemTime
```

Large or incremental files use the same strict pull protocol as other scoped
resources:

```solveig
withFileByteStream :
    Int -> Path ->
    (forall scope. Stream scope FsError Bytes -> IO FsError result) ->
    IO FsError result

withFileTextStream :
    Int -> Path ->
    (forall scope. Stream scope FsError String -> IO FsError result) ->
    IO FsError result

withFileLineStream :
    Int -> Path ->
    (forall scope. Stream scope FsError String -> IO FsError result) ->
    IO FsError result
```

The positive chunk size controls each host read, not a total file limit. Text
streams retain an incomplete UTF-8 scalar across byte chunks and report an
absolute byte offset for malformed input. Line streams accept LF and CRLF,
retain blank lines, and omit terminators. They allocate in proportion to the
longest individual line. The rank-2 callback prevents a stream from escaping
the handle that owns it; returning early, failing, raising a defect, or being
interrupted closes that handle.

Each runtime starts at the canonical project root and owns a logical working
directory. Relative paths for these operations and all file/handle operations
resolve against it. `setCurrentDirectory` updates only the runtime, never the
host process, so concurrent Solveig runtimes cannot change one another's path
resolution. The target must already be a directory.

`Path` values are validated with `pathFromString`; their representation is
not interchangeable with text. `listDirectory` returns immediate entry paths,
excludes `.` and `..`, and
sorts them lexically. A non-UTF-8 entry name is a wrapped `HostError
InvalidArgument`.
`createDirectory` creates exactly one directory and requires its parent to
exist. `doesFileExist` and `doesDirectoryExist` return `False` for a missing
path; other metadata failures remain typed errors. `getFileSize` reports bytes.
`getModificationTime` returns `SystemTime`; use
`systemTimeNanoseconds` only at an explicit numeric boundary.

Potentially blocking metadata, directory, and mutation calls use the bounded
IO worker pool. Every failure uses the constructor named for the attempted
operation and retains its path(s) plus the platform error message.

## Child processes

```solveig
process : String -> [String] -> Either ProcessError Process
runProcess : Process -> IO ProcessError ProcessResult
runProcessLimited : Process -> Int -> IO ProcessError ProcessResult
runProcessTimeout : Duration -> Process -> IO ProcessError (Maybe ProcessResult)
processExitCode : ProcessResult -> ExitCode
processStdoutBytes : ProcessResult -> Bytes
processStderrBytes : ProcessResult -> Bytes
processStdoutText : ProcessResult -> Either Utf8Error String
processStderrText : ProcessResult -> Either Utf8Error String
```

`process program arguments` validates a request. `runProcess processValue`
starts one child with the argument list exactly
as supplied: it does not invoke a shell, expand wildcards, or perform quoting.
The default request inherits the runtime-local environment and logical working
directory, closes standard input, and captures both output streams. Immutable
transformations configure it: `withWorkingDirectory` and
`inheritProcessWorkingDirectory`, `withEnvironment`, `withoutEnvironment`,
`clearProcessEnvironment`, `withStandardInput` or `withStandardInputText`, and
the capture/discard functions for stdout and stderr. Environment
transformations are ordered: setting a removed name restores it, and removing
a set name removes it. A discarded stream is still drained so it cannot fill a
pipe and block the child. Non-zero status is a successful `ProcessResult`, so
programs inspect it as normal data.

The exit code is opaque; use `isExitSuccess` or `exitCodeToInt` explicitly.
When a platform ends
the child without an exit code, such as a Unix signal, Solveig reports `-1`.
Captured streams are arbitrary `Bytes`. Decode explicitly with
`processStdoutText` or `processStderrText`; malformed UTF-8 remains
`Utf8Error` rather than being misreported as a process failure. Failure to
start, write input, read output, or wait returns
`ProcessHostError processValue hostError`. Use an absolute path
for a specific executable. A bare program name uses the host executable search
path; a relative path containing components resolves against the logical cwd.

`runProcess` has no library capture bound and is suitable only when both output
streams are trusted to fit memory. `runProcessLimited processValue
maximumPerStream` stores at most the non-negative maximum from each of stdout
and stderr. Either stream exceeding its limit terminates and reaps the child,
then returns `ProcessError`; an invalid limit fails before the child starts.
The maximum counts bytes and applies only to streams selected for capture.
`runProcessTimeout duration processValue` returns `Nothing` after interrupting,
terminating, and reaping the direct child; completion returns `Just result`.

Process execution uses the bounded blocking pool. Each active capture drains
stdout and stderr concurrently so one full pipe cannot deadlock the other, and
allocates packed language bytes only after the scheduler wakes the caller.
Interrupting the calling Solveig thread signals the owning worker, which
terminates and reaps the direct child before reusing that worker. Runtime
shutdown sends the same cancellation signal. Descendant processes started by
the child are outside this direct-child ownership contract.

## Program exit

```solveig
exitWith : forall a. ExitCode -> Task a
exitSuccess : forall a. Task a
exitFailure : forall a. Task a
```

`exitWith status` starts synchronous exit unwinding. Typed-error and defect
handlers do not catch it, while every installed `finally` or `bracket` cleanup
runs before the request escapes its green thread. A cleanup failure or defect
replaces the requested exit, just as it replaces any other outcome.

An exit escaping `main` interrupts all other green threads once and drains their
installed cleanup before returning the status to the host CLI. An exit from a
spawned thread ends only that thread; call `exitWith` from `main` to terminate
the program. `exitSuccess` requests zero and `exitFailure` requests one.

When shutdown completes, all worker-owned output handles—including open file
handles and the standard output streams—are flushed before library execution
returns or the CLI exits. Shutdown flush errors cannot replace an exit already
in progress. Active child-process capture is cancelled, terminated, and reaped
as part of that worker shutdown.

The `Int` is narrowed to the host's 32-bit process status, whose final
interpretation is operating-system dependent. Values from 0 through 255 are
the portable choice; Unix-like systems ordinarily expose only the low eight
bits to a parent process.

## Platform notes

File sharing and permission errors retain the host operating system's message
inside `HostError`, so wording differs by platform. Match `HostErrorKind`, not
that platform message, when control flow depends on the cause.
`ReadWriteMode` does not request file creation on any platform. Files are opened
in binary host mode and Solveig performs UTF-8 decoding and newline handling,
which keeps text and byte writes consistent across Unix and Windows. Paths
otherwise follow host syntax and filesystem semantics.

## Complete tutorial programs

The [file IO tutorial](tutorial/05-io-and-files.md) builds a complete program
that reads a text file and writes a report. The [concurrency
tutorial](tutorial/06-concurrency.md) then shows how to add bounded work without
mixing file ownership, error recovery, and scheduling into one unexplained
example. Both chapters link to runnable projects under `docs/tutorial/code/`.
