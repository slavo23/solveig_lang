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
            printStr (processStdout result)
```

The program is invoked directly, not through a shell. Arguments are separate
strings, which avoids shell quoting and expansion. `runProcessLimited` is the
opt-in variant for untrusted or potentially unbounded child output.
`processExitCode` returns opaque `ExitCode`; use `isExitSuccess` for the
common branch or `exitCodeToInt` at a host/protocol boundary.

## Cleanup patterns

Use the narrowest helper that owns the resource:

- `withFile` for handles;
- `withConnection` for outgoing sockets;
- `withListener` for listeners;
- `withThread` for child tasks;
- `bracket` for custom acquire/use/release lifetimes;
- `finally` when only an unconditional cleanup action is needed.

This rule becomes especially important once several actions can run
concurrently.

Next: [Concurrency](06-concurrency.md).
