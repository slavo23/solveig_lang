# 4. Cross the IO boundary

Log Audit's pure scanner has no way to read `logs/api.log`—and that is a
feature. Reading a file, printing, asking a clock, and starting a process depend
on the outside world, so Solveig represents them as `IO` values. The type
records both the failure type and the successful result:

```solveig
IO error result
```

`Task result` is the common alias `IO IOError result`. More specific modules
use more informative errors—for example, filesystem actions return
`IO FsError result` so a failure retains its `Path`.

**Skip ahead:** if typed IO and scoped resource management are familiar, read
[the complete Log Audit boundary](#the-complete-log-audit-boundary), then use
the time, randomness, and process sections as a practical reference.

## Sequencing actions with `do`

```solveig
main : Task ()
main = do
    printStr "What is your name? "
    name <- readLine
    printLine ("Hello, " <> name)
```

`<-` performs an action and binds its successful result. A line without `<-`
still performs an action, but discards its result. The final line determines
the result of the whole block. Constructing an `IO` value does not perform it;
the runtime begins interpreting the action only when it runs `main`.

Use `let` for a pure intermediate value:

```solveig
main : Task ()
main = do
    line <- readLine
    let message = "You typed " <> show line
    printLine message
```

Solveig is strict. The compiler lowers a `let` by evaluating its right-hand
side, storing the result, and only then compiling the body. Naming an expression
does not turn it into IO, but it also does not make that expression lazy. This
matters for performance and defects: a costly pure calculation or `error` in a
reached `let` happens even if the bound name is not used later.

## Choose the file abstraction

Solveig has two complementary file layers:

- `std.fs` is the normal application API. It accepts validated `Path` values,
  retains path context in `FsError`, offers strict and bounded reads, atomic
  writes, and scoped streams.
- `std.io` is the lower-level handle API. Use it when a program needs explicit
  open modes, buffering, mixed text/byte operations, or manual flushing.

There is only one high-level whole-file surface: `std.fs`. This avoids two
nearly identical APIs whose only difference is whether a path and its failure
context were accidentally erased.

## Construct paths deliberately

`Path` is not a synonym for `String`. `pathFromString` rejects empty spellings
and embedded NUL characters before an operating-system call is attempted:

```solveig
import std.path(Path, PathError, pathFromString)

parseRequestedPath : String -> Either PathError Path
parseRequestedPath = pathFromString
```

For a programmer-owned literal, it is reasonable to treat an invalid spelling
as a defect because it is a bug in the program rather than user input:

```solveig
requiredPath : String -> Path
requiredPath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error (show pathError)
```

Do not use that helper for untrusted text. Keep the `Either PathError Path` and
report or recover from `Left` explicitly.

Relative paths are resolved against the Solveig runtime's current directory,
initially the package root selected by `--project-root`.

## Read and write whole files

The ordinary strict operations are small and composable:

```solveig
import std.fs(FsError, readFile, writeFile)
import std.path(Path)

copyText : Path -> Path -> IO FsError ()
copyText source destination = do
    contents <- readFile source
    writeFile destination contents
```

- `readFile` decodes a complete UTF-8 file.
- `writeFile` creates or replaces a UTF-8 file.
- `appendFile` appends UTF-8 text.
- The corresponding `...Bytes` operations preserve arbitrary bytes.
- `writeFileAtomic` and `writeFileBytesAtomic` replace a destination through a
  synced sibling temporary file and one host rename.

The unrestricted readers are convenient when a complete trusted file
comfortably fits memory. At a trust boundary, choose a total byte policy:

```solveig
readUntrustedConfig : Path -> IO FsError String
readUntrustedConfig path = std.fs.readFileLimited path 1048576
```

That one-megabyte bound is checked while reading; it is not a racy metadata
pre-check. The host reads at most one byte past the limit to distinguish an
exact fit from overflow. When input may not fit memory at all, use a scoped
stream instead of inventing a very large limit.

## The complete Log Audit boundary

The checked [Log Audit source](../../examples/log_audit/src/main.sv) converts a
filesystem error only where that specific action joins the application's
`Task` boundary:

```solveig
import std.fs(FsError(..))
import std.path(Path, pathFromString)

fsToIO : FsError -> IOError
fsToIO (FsIOError _ ioError) = ioError
fsToIO error = IOError InvalidArgument "filesystem" (show error)

requiredPath : String -> Path
requiredPath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error (show pathError)

auditFile : Chan AuditResult -> Path -> Task ()
auditFile results path = do
    contents <- mapError fsToIO (std.fs.readFile path)
    send results (Audited (analyzeLog contents))
```

`mapError` changes only the error channel; it does not catch a failure or turn
it into a successful value. `auditOne` separately uses `catchError` to turn
the expected missing-file case into `MissingLog` and re-raises other
`IOError` values. Translation and recovery are different decisions.

At the outer boundary, the program replaces one report and reads it back:

```solveig
let reportPath = requiredPath "output/report.txt"
mapError fsToIO (std.fs.writeFile reportPath (renderReport summary))
report <- mapError fsToIO (std.fs.readFile reportPath)
printStr report
```

Reading the report back is not required for writing. The application does it so
the terminal displays the exact artifact produced by the filesystem action.

## Handle-based IO

Whole-file helpers are best for strict inputs. Handles support incremental
reads, appending, and explicit buffering:

```solveig
import std.io(AppendMode, hFlush, hPutStrLn, withFile)
import std.path(Path, pathToString)

writeLog : Path -> String -> Task ()
writeLog path message =
    withFile (pathToString path) AppendMode (fun handle => do
        hPutStrLn handle message
        hFlush handle)
```

`withFile` opens the handle, passes it to the callback, and closes it after
success, typed failure, a defect, or interruption. Prefer it to manual
`openFile`/`hClose` when the handle does not need to cross a larger ownership
boundary. Garbage collection never closes handles.

The modes are `ReadMode`, `WriteMode`, `AppendMode`, and `ReadWriteMode`.
Buffer modes are `NoBuffering`, `LineBuffering`, and `BlockBuffering (Maybe
Int)`. `hGetLine` reads one line, `hGetContents` strictly reads all remaining
text, `hGetBytes` reads at most a requested byte count, and `hIsEOF` checks for
EOF. Text operations validate UTF-8; byte operations do not decode.

## Stream a large file

A file stream performs one bounded host read only when the consumer asks for
the next chunk:

```solveig
import std.bytes(bytesLength)
import std.fs(FsError, withFileByteStream)
import std.path(Path)
import std.stream(Stream, fold, take)

countThreeChunks : forall scope. Stream scope FsError Bytes -> IO FsError Int
countThreeChunks stream =
    fold
        (fun total chunk => total + bytesLength chunk)
        0
        (take 3 stream)

inspectPrefix : Path -> IO FsError Int
inspectPrefix path = withFileByteStream 65536 path countThreeChunks
```

`65536` is the maximum size of one pull, not a maximum file size. The stream
does no background read-ahead, and `take 3` never requests a fourth chunk.
The callback accepts a stream with a fresh `scope`; the type checker prevents
that stream, or a transformed stream derived from it, from escaping after the
file closes.

A resource stream is single-consumer. Continue with the remaining stream
returned by each pull—normally through `fold`, `forEach`, or another
combinator—and never pull the same stream value concurrently. Cleanup runs if
the callback succeeds, returns a typed error, raises a defect, or is
interrupted. The [strict-stream guide](../streaming.md) develops that protocol
in detail.

`withFileTextStream` preserves incomplete UTF-8 scalars across byte-chunk
boundaries. `withFileLineStream` yields lines without terminators, accepts LF
and CRLF, and retains blank lines. A chunk bound limits each host read; one
exceptionally long line can still require proportional memory.

## Recover from filesystem errors

Missing data is not represented by an empty string. Recover only from the
failure intended by the program and re-raise everything else:

```solveig
import std.fs(FsError(..), readFile)
import std.path(Path)

readOrDefault : Path -> String -> IO FsError String
readOrDefault path fallback =
    catchError
        (readFile path)
        (fun fileError =>
            case fileError of
                FsIOError _ ioError =>
                    if ioErrorKind ioError == DoesNotExist
                        then pureIO fallback
                        else failIO fileError
                | FsInvalidUtf8 _ _ => failIO fileError)
```

Matching `DoesNotExist` avoids hiding permission failures, malformed UTF-8,
resource exhaustion, or unrelated I/O errors. `FsInvalidUtf8` preserves both
the path and the invalid byte offset.

## Filesystem state and atomic replacement

`std.fs` also provides directory and metadata operations:

- `getCurrentDirectory` and `setCurrentDirectory`;
- `doesFileExist` and `doesDirectoryExist`;
- `listDirectory` and `createDirectory`;
- `removeFile` and `renameFile`;
- `getFileSize` and `getModificationTime`.

`getModificationTime` returns `SystemTime`, keeping wall-clock values distinct
from monotonic `Instant` readings and integer units. The current directory is
runtime state shared by the program. Changing it while other threads resolve
relative paths can produce surprising results, so concurrent code should
prefer stable `Path` values over frequent directory changes.

An ordinary `writeFile` can expose a partially written new file if the process
or host fails mid-write. Use `writeFileAtomic` for configuration, cache, and
state files whose readers must observe either the old complete contents or the
new complete contents. Atomic replacement does not make concurrent logical
updates transactional: two writers can still compute from the same old state
and the later rename wins.

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

Environment values belong to the runtime-local environment snapshot inherited
by child processes. They are text, not secrets management; avoid printing
tokens merely because retrieval is convenient.

For a required setting, preserve absence as `EnvError` instead of inventing a
sentinel value:

```solveig
import std.env(EnvError, requireEnv)

apiToken : IO EnvError String
apiToken = requireEnv "SERVICE_TOKEN"
```

`loadEnvFile` reads at most 64 KiB of strict `NAME=VALUE` assignments and does
not overwrite variables already present in the runtime environment. It is a
small dotenv reader, not a shell: it performs no interpolation, command
substitution, or sourcing. That makes configuration loading predictable and
keeps a deployment-provided variable authoritative over a local file.

## Time: duration, monotonic time, and wall time

Time has three distinct public types because the values answer different
questions:

- `Duration` is a signed span. Construct one with `milliseconds`, `seconds`,
  `microseconds`, or `nanoseconds`.
- `Instant` is a reading from a runtime-local monotonic clock. Use it for
  elapsed time, deadlines, and performance measurements.
- `SystemTime` is wall-clock time relative to the Unix epoch. Use it for stored
  timestamps and interoperability.

This prevents accidentally passing “seconds since 1970” to `sleep`, or
subtracting a wall-clock value from a monotonic reading. A real batch operation
can be measured without choosing an integer unit until presentation:

```solveig
import std.time(durationToMilliseconds, measure)

reportDuration : forall a. Task a -> Task a
reportDuration action = do
    (elapsed, value) <- measure action
    printLine $ "completed in " <> show (durationToMilliseconds elapsed) <> " ms"
    pureIO value
```

`measure` uses the monotonic clock, so a user or network time correction cannot
make elapsed time jump backward. Use `getSystemTime` and
`systemTimeNanoseconds` only when an epoch timestamp is required. Duration
constructors saturate on integer overflow; unit conversions return whole units
and truncate toward zero. `sleep` rounds a positive sub-millisecond duration up
to one host millisecond and returns immediately for a non-positive duration.

## Reproducible and secure randomness

`std.random` deliberately separates reproducible pseudorandomness from host
cryptographic randomness. An immutable `StdGen` is explicit state:

```solveig
import std.random(mkStdGen, randomRange)

twoDice : StdGen -> Maybe ((Int, Int), StdGen)
twoDice initial =
    case randomRange 1 6 initial of
        Nothing => Nothing
        | Just (first, afterFirst) =>
            case randomRange 1 6 afterFirst of
                Nothing => Nothing
                | Just (second, afterSecond) => Just ((first, second), afterSecond)

replay : Maybe ((Int, Int), StdGen)
replay = twoDice $ mkStdGen 20260905
```

The inclusive bounds are checked: reversed bounds return `Nothing`. Passing the
returned generator to the next draw makes state progression visible; reusing
the original generator intentionally repeats the same draw. A fixed seed gives
the same sequence across runs, which is ideal for simulations and regression
tests. `newStdGen` chooses a fresh runtime-local seed for non-security uses, but
it is still not a cryptographic generator and there is no global mutable
generator shared between threads.

For session identifiers, password-reset tokens, encryption material, or any
value whose unpredictability matters, use `secureRandomBytes`. It obtains the
requested bytes from the operating system's cryptographic source and returns
`IO SecureRandomError Bytes`. It is neither seeded nor reproducible. Keeping
that operation in a separate API and error type makes a security boundary
visible in code review.

## Run child processes without a shell

`std.process` passes a program and an argument list directly to the host process
API. It does not invoke a shell, expand globs, interpolate variables, or treat
spaces and punctuation in an argument as syntax. This is both easier to reason
about and safer for user-provided arguments:

```solveig
import std.process(*)

data CommandError =
    CommandProcess ProcessError
    | CommandFailed ExitCode
    | CommandOutput Utf8Error

gitStatus : IO CommandError String
gitStatus =
    case process "git" ["status", "--short"] of
        Left error => failIO $ CommandProcess error
        | Right request => do
            result <- mapError CommandProcess $ runProcessLimited request 1048576
            if isExitSuccess $ processExitCode result
                then case processStdoutText result of
                    Left error => failIO $ CommandOutput error
                    | Right output => pureIO output
                else failIO $ CommandFailed $ processExitCode result
```

`process` validates the executable and arguments before host work. The default
request inherits the runtime working directory and environment, closes stdin,
and captures stdout and stderr as bytes. Immutable builders configure a `Path`
working directory, explicit environment changes, strict byte/text stdin, and
capture or discard policy. Output remains binary until
`processStdoutText`/`processStderrText` explicitly validate UTF-8.

`runProcess` has unbounded capture and is suitable only when the child is
trusted to produce small output. Application code should normally choose
`runProcessLimited`; its limit applies independently to each captured output
stream. `runProcessTimeout duration request` returns `Nothing` after expiry and
does not abandon a zombie: the runtime closes pipes, terminates the direct
child, and waits for it.

For a large or interactive child, use `withProcess`. Its positive chunk size
bounds each stdin write and stdout/stderr pull, while the rank-2 callback keeps
the live handle from escaping its scope. If both captured streams can fill,
drain them concurrently:

```solveig
import std.bytes(bytesLength)
import std.process(*)
import std.stream(Stream, fold)
import std.task(concurrently)

byteCount : forall scope. Stream scope ProcessError Bytes -> IO ProcessError Int
byteCount = fold (fun total chunk => total + bytesLength chunk) 0

drainChild : forall scope.
    ProcessHandle scope -> IO ProcessError (Int, Int, ExitCode)
drainChild child = do
    (stdoutBytes, stderrBytes) <-
        concurrently (byteCount $ processStdout child) (byteCount $ processStderr child)
    status <- waitProcess child
    pureIO (stdoutBytes, stderrBytes, status)
```

Sequentially draining stdout and then stderr can deadlock: the child may block
writing a full stderr pipe while the parent waits for stdout EOF. The scoped
API therefore exposes both streams and `concurrently` supplies the required
two-sided drain. Leaving the callback by success, typed failure, defect, or
interruption terminates a still-running child and unconditionally reaps it.

Use `withStandardInput` for a finite request body. Use `pipeStandardInput` plus
`writeProcessStdin` and `closeProcessStdin` for an interactive protocol, and
keep each write no larger than the `withProcess` chunk size. `terminateProcess`
is explicit early shutdown; ordinary scope cleanup provides the safety net.

## Deliberate error: mixing error channels

If `auditFile` calls `std.fs.readFile path` directly inside `Task ()`, the
compiler cannot unify `FsError` with `IOError`. The repair is an explicit
policy—keep `IO FsError` in the caller, or use `mapError fsToIO`—not an
unchecked exception conversion.

## Checkpoint

Run Log Audit and inspect `output/report.txt`:

```sh
cargo run -- --project-root examples/log_audit
```

Then answer these boundary questions:

1. Which definitions are pure and can be evaluated without a filesystem?
2. Where does `FsError` become `IOError`?
3. Which missing-file failure becomes domain data?
4. Which resource-owning examples in this chapter use a rank-2 callback?

## Exercises

1. In a scratch copy, replace `readFile` with `readFileLimited path 1024`.
   Explain what the bound protects.
2. Write a pure function that chooses an output path, leaving the write itself
   in `main`.
3. Decide whether a large child-process output should use
   `runProcessLimited` or `withProcess`, and state the memory policy.
4. Use a fixed `StdGen` seed to generate a repeatable test sequence; explain
   why the same code must not generate a password-reset token.

## Recap

- `IO error result` keeps both the failure and success type visible.
- `Task a` is `IO IOError a`, not a separate untyped effect system.
- `std.fs` owns path-aware file operations; `std.io` owns lower-level
  handles.
- Strict reads need a trust argument, a size limit, or a streaming design.
- Scoped callbacks close files, streams, sockets, and processes on every exit.
- `Duration`, `Instant`, and `SystemTime` represent different domains.
- Seeded pseudorandomness and host cryptographic randomness are separate APIs.
- Child processes are direct, binary-safe, bounded when requested, and reaped.

Next: [Organize and test an application](04-abstraction.md).
