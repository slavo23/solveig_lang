# Input and output

Solveig's `Task a` is `IO IOError a`: an inert description that the runtime
performs when it runs `main`. Blocking file and standard-input operations run
on the bounded IO worker pool, so a blocked handle does not block lightweight
threads or the scheduler. Open files are distributed across that pool and all
operations for one handle stay FIFO.

## Handles and ownership

`Handle` is an abstract compiler-known type. A value is only an opaque identity
into one runtime's host-resource table; a Solveig program cannot inspect or
construct one. Handles are not heap objects.

**GC never closes a handle.** Use `withFile` for normal code, or call `hClose`
explicitly when ownership must cross a larger scope. `withFile` is defined via
`bracket`, so it closes after success, a typed `IOError`, a defect, or thread
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

Using a closed handle raises the error for the attempted operation, such as
`ReadHandleError`, `WriteHandleError`, or `FlushHandleError`. Handle identities
are not reused within a runtime, so a stale identity cannot alias a later file.

## Opening and closing

```solveig
data IOMode = ReadMode | WriteMode | AppendMode | ReadWriteMode

openFile : String -> IOMode -> Task Handle
hClose : Handle -> Task ()
hFlush : Handle -> Task ()
```

`openFile` resolves relative paths against the logical working directory and raises
`OpenHandleError path message` on failure.

- `ReadMode` opens an existing readable file.
- `WriteMode` creates the file if necessary and truncates an existing file.
- `AppendMode` creates the file if necessary and writes at the end.
- `ReadWriteMode` opens an existing file for reading and writing without
  truncating or creating it.

`hClose` first flushes buffered output, closes the logical handle, and raises
`CloseHandleError message` if either step fails. Closing an already closed
handle is an error. `hFlush` writes pending output to the host stream and raises
`FlushHandleError message` on failure. Normal runtime shutdown makes a
best-effort flush, but it cannot report failures to a finished program and is
not a substitute for `withFile` or `hClose`.

## Text input

```solveig
hGetLine : Handle -> Task String
hGetContents : Handle -> Task String
hIsEOF : Handle -> Task Bool
```

Text is decoded strictly as UTF-8. Invalid UTF-8, a handle opened without read
permission, or a host read failure raises `ReadHandleError message`. `hGetLine`
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
without write permission, a closed handle, or a host write failure raises
`WriteHandleError message`. Success means the text reached the selected
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
or host-unrepresentable size raises `SetBufferingError message`.

Changing buffering first flushes pending writes and preserves the logical read
position. A failure, including use of a closed handle, raises
`SetBufferingError message`. Terminal `stdout` and `stderr` default to line
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
over buffering. Failures use `ReadHandleError` or `WriteHandleError`, including
when the corresponding standard handle was explicitly closed.

## Byte input and output

```solveig
readFileBytes : String -> Task Bytes
writeFileBytes : String -> Bytes -> Task ()
hGetBytes : Handle -> Int -> Task Bytes
hPutBytes : Handle -> Bytes -> Task ()
```

These operations transfer arbitrary bytes without encoding or newline
translation. `readFileBytes` strictly reads and allocates the entire file;
`writeFileBytes` creates or replaces a file just like `writeFile`, but writes
the supplied bytes unchanged. They bypass handle buffering and raise
`ReadFileError path message` or `WriteFileError path message` on host failure.

`hGetBytes handle maximum` reads at most the non-negative `maximum`. It returns
fewer bytes at EOF and returns empty bytes when called at EOF or with zero.
Regular files fill the requested count unless EOF is reached; `stdin` may
return a shorter chunk when that is what the embedding host stream makes
available. A negative or host-unrepresentable maximum, a non-readable or
closed handle, and host failures raise `ReadHandleError message`.

`hPutBytes` uses precisely the same buffer as text output. Under line
buffering, a byte value `10` triggers a flush; block-buffer sizes count bytes.
Success means the bytes reached the language buffer, so use `hFlush` when the
host must observe them immediately. A non-writable or closed handle and host
failures raise `WriteHandleError message`.

Text and byte operations can be mixed on one handle without changing its
logical position. Text input still decodes strictly: invalid UTF-8 raises the
operation's read error. Byte input never decodes. Convert explicitly with
`bytesToString`; it returns `Err (InvalidUtf8 message)` rather than panicking or
replacing malformed input.

## Environment and program identity

```solveig
getEnv : String -> Task (Maybe String)
setEnv : String -> String -> Task ()
getEnvironment : Task [Pair String String]
getProgName : Task String
```

The runtime snapshots UTF-8 host environment entries at startup. `setEnv`
changes this runtime-local copy, and subsequent environment reads observe the
change; it does not mutate the host process's global environment. Names must be
non-empty and contain neither `=` nor NUL,
and values cannot contain NUL. Invalid inputs raise `GetEnvError` or
`SetEnvError`. `getEnvironment` is sorted by variable name for deterministic
iteration.

Until package manifests define an executable name, `getProgName` returns the
project root's final directory component. Non-UTF-8 host entries are omitted
because Solveig `String` is UTF-8.

## Clocks

```solveig
getMonotonicTime : Task Int
getSystemTime : Task Int
```

Both operations return nanoseconds. `getMonotonicTime` measures elapsed time
from an arbitrary origin local to one runtime and never decreases; subtract
two readings to measure a duration. Its absolute value has no relationship to
a calendar clock. `getSystemTime` is a signed count since the Unix epoch and
can move when the host clock is adjusted. Values outside the `Int` nanosecond
range saturate instead of wrapping.

The operations execute directly on the scheduler thread: neither blocks nor
uses an IO worker. Normal execution reads the host clocks. Captured execution
and `SOLVEIG_SCHED_DETERMINISTIC=1` expose the scheduler's logical elapsed time
for both operations, with the virtual system clock beginning at the Unix
epoch. A virtual `sleep 5` therefore advances both readings by exactly
5,000,000 nanoseconds. This makes clock-dependent program tests exact while
keeping the same action and scheduler paths as normal execution.

## Pseudorandom generation

```solveig
mkStdGen : Int -> StdGen
newStdGen : Task StdGen
randomInt : StdGen -> Pair Int StdGen
randomRange : Int -> Int -> StdGen -> Pair Int StdGen
```

`StdGen` is opaque, immutable SplitMix64 state. `mkStdGen` makes a repeatable
generator from any `Int`; the sequence for a fixed seed is part of Solveig's
behavior. Each pure draw returns both its result and the next generator. Reuse
the same input generator to replay the same result, or thread the returned
state through later draws:

```solveig
case randomRange 1 6 (mkStdGen 42) of
    MkPair die next => randomRange 1 6 next
```

`randomInt` covers the complete signed `Int` range. `randomRange lower upper`
uses inclusive bounds and unbiased rejection sampling. It raises an
`Error "randomRange lower bound must be less than or equal to upper bound"`
defect when the bounds are reversed. Because rejection may consume another
word, callers must treat the returned generator—not a guessed step count—as
the next state.

Only `newStdGen` performs IO. It draws a distinct generator seed from one
runtime-local SplitMix stream initially seeded from the runtime's system clock.
Captured and deterministic execution begin that stream at seed zero, so tests
remain exact. Clock-derived seeds can repeat across runtimes and are not host
entropy. These APIs are pseudorandom and must not be used for keys, tokens,
nonces, passwords, or any other cryptographic purpose.

## Filesystem and logical working directory

```solveig
getCurrentDirectory : Task String
setCurrentDirectory : String -> Task ()
doesFileExist : String -> Task Bool
doesDirectoryExist : String -> Task Bool
listDirectory : String -> Task [String]
createDirectory : String -> Task ()
removeFile : String -> Task ()
renameFile : String -> String -> Task ()
getFileSize : String -> Task Int
getModificationTime : String -> Task Int
```

Each runtime starts at the canonical project root and owns a logical working
directory. Relative paths for these operations and all file/handle operations
resolve against it. `setCurrentDirectory` updates only the runtime, never the
host process, so concurrent Solveig runtimes cannot change one another's path
resolution. The target must already be a directory.

`listDirectory` returns immediate entry names, excludes `.` and `..`, and
sorts them lexically. A non-UTF-8 entry name is a `ListDirectoryError`.
`createDirectory` creates exactly one directory and requires its parent to
exist. `doesFileExist` and `doesDirectoryExist` return `False` for a missing
path; other metadata failures remain typed errors. `getFileSize` reports bytes.
`getModificationTime` reports signed nanoseconds since the Unix epoch and
raises its typed error if the host value cannot fit `Int`.

Potentially blocking metadata, directory, and mutation calls use the bounded
IO worker pool. Every failure uses the constructor named for the attempted
operation and retains its path(s) plus the platform error message.

## Child processes

```solveig
data ProcessResult = MkProcessResult Int String String

runProcess : String -> [String] -> Task ProcessResult
processExitCode : ProcessResult -> Int
processStdout : ProcessResult -> String
processStderr : ProcessResult -> String
```

`runProcess program arguments` starts one child with the argument list exactly
as supplied: it does not invoke a shell, expand wildcards, or perform quoting.
The child inherits the runtime-local environment and logical working directory,
while standard input is closed. Standard output and error are piped and
strictly captured to completion. Non-zero child status is a successful
`ProcessResult`, so programs can inspect and handle it as normal data.

The exit code is the platform's ordinary integer status. When a platform ends
the child without an exit code, such as a Unix signal, Solveig reports `-1`.
Both captured streams must be UTF-8 because `ProcessResult` exposes `String`;
invalid output, failure to start or wait for the program, and other host
capture failures raise `RunProcessError program message`. Use an absolute path
for a specific executable. A bare program name uses the host executable search
path; a relative path containing components resolves against the logical cwd.

Process execution uses the bounded blocking pool and allocates captured strings
only after the scheduler wakes the calling green thread. Capture is deliberately
strict and memory-bounded only by runtime/host limits; streaming subprocess IO
is a later feature and should be used instead for unbounded output once added.
Interrupting the Solveig caller stops waiting for the result, but the underlying
blocking host request may finish in the background before its worker is reused.

## Program exit

```solveig
exitWith : forall a. Int -> Task a
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
in progress. A blocking host operation that cannot be cancelled, such as the
capture-only `runProcess`, may still delay worker-pool shutdown even though its
green thread has stopped waiting.

The `Int` is narrowed to the host's 32-bit process status, whose final
interpretation is operating-system dependent. Values from 0 through 255 are
the portable choice; Unix-like systems ordinarily expose only the low eight
bits to a parent process.

## Platform notes

File sharing and permission errors retain the host operating system's message
inside the operation-specific `IOError`, so wording differs by platform.
`ReadWriteMode` does not request file creation on any platform. Files are opened
in binary host mode and Solveig performs UTF-8 decoding and newline handling,
which keeps text and byte writes consistent across Unix and Windows. Paths
otherwise follow host syntax and filesystem semantics.
