# 5. IO and files

Pure expressions describe values. Reading a file, printing, asking the clock,
and starting a process depend on the outside world, so Solveig represents them
as `IO` values. The type records both the failure type and the successful
result:

```solveig
IO error result
```

`Task result` is the common alias `IO HostError result`. More specific modules
use more informative errors—for example, filesystem actions return
`IO FsError result` so a failure retains its `Path`.

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

Pure `let` bindings are evaluated when demanded. Naming an expression neither
turns it into IO nor forces it eagerly.

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

## A complete file program

The tutorial's [`file_summary`](code/file_summary/) project reads four lines,
writes a summary, and prints it. Its filesystem error is converted to
`HostError` only where it joins the Prelude printing actions in `main`:

```solveig
import std.fs(FsError(..))
import std.list(length)
import std.path(Path, pathFromString)
import std.text(lines, trimAscii)

fsToHost : FsError -> HostError
fsToHost (FsHostError _ hostError) = hostError
fsToHost fileError = HostError InvalidArgument "filesystem" (show fileError)

requiredPath : String -> Path
requiredPath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error (show pathError)

nonEmpty : String -> Bool
nonEmpty text = text /= ""

main : Task ()
main = do
    contents <- mapError fsToHost (std.fs.readFile (requiredPath "input/readings.txt"))
    let readings = filter nonEmpty (lines (trimAscii contents))
    let report = "readings: " <> show (length readings) <> "\n"
    mapError fsToHost (std.fs.writeFile (requiredPath "summary.txt") report)
    printStr report
```

Run it from the repository root:

```sh
solveig --project-root docs/tutorial/code/file_summary
```

The output is:

```text
readings: 4
```

`mapError` changes only the error channel; it does not catch a failure or turn
it into a successful value. A larger application would often define its own
sum error type instead of collapsing `FsError` into `HostError`.

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
                FsHostError _ hostError =>
                    if hostErrorKind hostError == DoesNotExist
                        then pureIO fallback
                        else failIO fileError
                | FsInvalidUtf8 _ _ => failIO fileError)
```

Matching `DoesNotExist` avoids hiding permission failures, malformed UTF-8,
resource exhaustion, or unrelated host errors. `FsInvalidUtf8` preserves both
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

Next: [Errors and resource safety](06-errors-and-resources.md).
