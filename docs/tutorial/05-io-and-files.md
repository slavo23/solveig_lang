# 5. IO and files

Pure functions always produce the same result for the same input. Reading a
file, printing, asking the clock, and starting a process depend on the outside
world, so Solveig represents them as `IO` values.

## `Task` and `do`

For ordinary system IO, use `Task a`, which is `IO HostError a`:

```solveig
main : Task ()
main = do
    printStr "What is your name? "
    name <- readLine
    printLine ("Hello, " <> name)
```

`<-` runs an action and binds its successful result. A line without `<-` is
still an action; its result is discarded. The last line determines the result
of the whole block.

Use `let` for pure computation inside `do`:

```solveig
main : Task ()
main = do
    line <- readLine
    let message = "You typed " <> show line
    printLine message
```

Pure `let` bindings are evaluated when their value is demanded. Merely naming
a computation does not turn it into IO or force it eagerly.

## Read and write whole files

The normal API is deliberately small:

```solveig
import std.io(*)

copyText : String -> String -> Task ()
copyText source destination = do
    contents <- readFile source
    writeFile destination contents
```

- `readFile` reads a complete UTF-8 text file.
- `writeFile` replaces a file.
- `appendFile` adds text at the end.
- `readFileBytes` and `writeFileBytes` work with arbitrary bytes.

You do not need to choose a size limit for ordinary use. The `...Limited`
variants are opt-in resource policies for trust boundaries. For example, a
service that reads a path supplied by a remote client may choose:

```solveig
readUntrustedConfig : String -> Task String
readUntrustedConfig path = readFileLimited path 1048576
```

That program has made a one-megabyte policy explicit. A local command-line tool
can simply use `readFile`.

## A complete file program

The tutorial's [`file_summary`](code/file_summary/) project reads four lines,
writes a summary, and prints it:

```solveig
import std.io(*)
import std.list(length)
import std.text(lines, trimAscii)

nonEmpty : String -> Bool
nonEmpty text = text /= ""

main : Task ()
main = do
    contents <- readFile "input/readings.txt"
    let readings = filter nonEmpty (lines (trimAscii contents))
    let report = "readings: " <> show (length readings) <> "\n"
    writeFile "summary.txt" report
    printStr report
```

Run it from the repository root:

```sh
solveig --project-root docs/tutorial/code/file_summary
```

Paths are resolved against the Solveig runtime's current directory, initially
the package root selected by `--project-root`.

## Handle-based IO

Whole-file helpers are best for small and medium strict inputs. Handles support
incremental reads, appending, and explicit buffering:

```solveig
import std.io(*)

writeLog : String -> Task ()
writeLog message =
    withFile "application.log" AppendMode (fun handle => do
        hPutStrLn handle message
        hFlush handle)
```

`withFile` opens the handle, passes it to the body, and closes it after success,
typed failure, or interruption. Prefer it to manual `openFile`/`hClose` when the
handle does not need to escape.

The modes are `ReadMode`, `WriteMode`, `AppendMode`, and `ReadWriteMode`.
Buffer modes are `NoBuffering`, `LineBuffering`, and `BlockBuffering (Maybe
Int)`. `hGetLine` reads one line, `hGetContents` reads the remaining text,
`hGetBytes` reads at most a requested chunk, and `hIsEOF` checks for EOF.

## Stream a large file

Manual handle loops are useful at a low level, but ordinary incremental
consumers use the shared strict stream API. This example reads at most three
chunks, counts their bytes, and closes the file immediately:

```solveig
import std.bytes(bytesLength)
import std.io(withFileByteStream)
import std.stream(Stream, fold, take)

countThreeChunks : forall scope. Stream scope HostError Bytes -> Task Int
countThreeChunks stream =
    fold
        (fun total chunk => total + bytesLength chunk)
        0
        (take 3 stream)

main : Task ()
main = do
    byteCount <- withFileByteStream 65536 "archive.bin" countThreeChunks
    printLine ("inspected " <> show byteCount <> " bytes")
```

`65536` is the maximum size of one chunk, not a maximum file size. The stream
does no background read-ahead, and `take 3` never requests a fourth chunk. The
callback accepts a stream with a fresh `scope`; the type checker prevents that
stream from being returned and used after the file closes. Cleanup also runs
if the callback returns a typed error, raises a defect, or is interrupted.

A resource stream is single-consumer. Use the remaining stream returned by
each pull—normally through `fold`, `forEach`, or another combinator—and do not
pull the same value concurrently. The [strict-stream guide](../streaming.md)
explains the protocol, finite adapters, buffering, failures, and cancellation.

## Recover from file errors

Low-level whole-file operations fail through `HostError`, not by returning an
empty string. Match its stable kind instead of a platform-specific message:

```solveig
import std.io(readFile)

readOrDefault : String -> String -> Task String
readOrDefault path fallback =
    catchError
        (readFile path)
        (fun hostError =>
            if hostErrorKind hostError == DoesNotExist then pureIO fallback
            else failIO hostError)
```

Matching a constructor lets you recover only from the condition you intended.
Re-raising `other` prevents unrelated failures from being hidden.

## Filesystem operations

`std.fs` provides:

- `getCurrentDirectory` and `setCurrentDirectory`;
- `doesFileExist` and `doesDirectoryExist`;
- `listDirectory` and `createDirectory`;
- `removeFile` and `renameFile`;
- `getFileSize` and `getModificationTime`.

It also owns the high-level file operations. These accept `Path`, retain path
context in `FsError`, and report invalid UTF-8 separately from host failures:

```solveig
readConfig : Path -> IO FsError String
readConfig = std.fs.readFile

saveConfig : Path -> String -> IO FsError ()
saveConfig = std.fs.writeFileAtomic
```

Use `readFile` for an ordinary trusted file, `readFileLimited` at an untrusted
finite-input boundary, and `withFileByteStream`, `withFileTextStream`, or
`withFileLineStream` when the input may not fit memory. A stream's chunk size
bounds each read; it is not a total download or file limit.

They return `IO FsError a`, retaining filesystem path context around the host
cause. Filesystem operations accept an opaque `Path`; validate external text
with `pathFromString` and convert back only with `pathToString`.
`getModificationTime` returns `SystemTime`, keeping wall-clock values
distinct from monotonic `Instant` readings and integer units. The current
directory is runtime state shared by the program, so
changing it concurrently can make relative paths surprising. In
concurrent code, prefer stable paths over frequent directory changes.

## Environment and arguments

`std.env.getArgs` returns arguments after the CLI's `--` separator:

```solveig
import std.env(getArgs, getEnvOr)

main : Task ()
main = do
    arguments <- getArgs
    color <- getEnvOr "APP_COLOR" "blue"
    printLine ("color: " <> color <> ", args: " <> show arguments)
```

Run it with:

```sh
solveig --project-root path/to/app -- first second
```

`getEnv` returns `Maybe String`; `requireEnv` uses the typed `EnvError` channel.
`loadEnvFileIfExists` can load a local dotenv-style file without treating a
missing file as an error. Do not print secrets read from the environment.

## Child processes

`std.process.runProcess processValue` waits for a validated child-process
request and returns its exit code, stdout, and stderr:

```solveig
import std.process(*)

main : Task ()
main =
    case process "git" ["--version"] of
        Left processError => printLine (show processError)
        | Right processValue => do
            result <- runProcess processValue
            case processStdoutText result of
                Left utf8Error => printLine (show utf8Error)
                | Right output => printStr output
```

The program is invoked directly, not through a shell. Arguments are separate
strings, which avoids shell quoting and expansion. `runProcessLimited` is the
opt-in variant for untrusted or potentially unbounded child output.
Captured output is `Bytes`, because programs are allowed to emit arbitrary
binary data; the text accessor performs explicit UTF-8 validation. Use
`withWorkingDirectory`, `withEnvironment`, `withoutEnvironment`, and
`withStandardInputText` to derive a request without mutating process-global
state. `inheritProcessWorkingDirectory` restores the default after a working
directory was configured. `discardStandardError` drains stderr without
retaining it.
`runProcessTimeout duration request` returns `Nothing` only after the direct
child has been terminated and reaped.
`processExitCode` returns opaque `ExitCode`; use `isExitSuccess` for the
common branch or `exitCodeToInt` at a host/protocol boundary.

For a large-output or interactive child, use `withProcess`. Its callback owns
a live `ProcessHandle` whose type-level scope prevents it and its streams from
escaping:

```solveig
import std.bytes(bytesLength)
import std.process(*)
import std.stream(Stream, fold)
import std.task(concurrently)

countBytes : forall scope. Stream scope ProcessError Bytes -> IO ProcessError Int
countBytes = fold (fun total chunk => total + bytesLength chunk) 0

measureOutputs : Process -> IO ProcessError (Int, Int, ExitCode)
measureOutputs request =
    withProcess 32768 request (fun child => do
        (stdoutBytes, stderrBytes) <- concurrently
            (countBytes (processStdout child))
            (countBytes (processStderr child))
        status <- waitProcess child
        pureIO (stdoutBytes, stderrBytes, status))
```

The positive size bounds every output pull. No output is read ahead. Drain
stdout and stderr concurrently when both might be large; reading one to EOF
first can allow the other operating-system pipe to fill and stop the child.
If one stream is irrelevant, select `discardStandardOutput` or
`discardStandardError`; the runtime drains it without retaining bytes.

Interactive stdin is explicit. Derive the request with `pipeStandardInput`,
then send chunks no larger than the `withProcess` size. Each write waits until
the host pipe has accepted and flushed that chunk, providing backpressure.
Call `closeProcessStdin` to deliver EOF; a repeated successful close is safe.
`waitProcess` waits without consuming unread output, while `terminateProcess`
stops and reaps the direct child and leaves captured streams readable to EOF.
Returning from the callback for any reason closes all pipes and reaps a
still-running direct child. Wrap the whole `withProcess` action in `timeout`
when the complete session needs a deadline.

## Cleanup patterns

Use the narrowest helper that owns the resource:

- `withFile` for handles;
- `withTcpConnection` for outgoing TCP streams;
- `withTcpListener` and `withAcceptedTcpConnection` for TCP servers;
- `withUdpSocket` for UDP datagrams;
- `withProcess` for live child pipes;
- `withThread` for child tasks;
- `bracket` for custom acquire/use/release lifetimes;
- `finally` when only an unconditional cleanup action is needed.

This rule becomes especially important once several actions can run
concurrently.

Next: [Concurrency](06-concurrency.md).
