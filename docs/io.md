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

`openFile` resolves relative paths against the project root and raises
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

## Platform notes

File sharing and permission errors retain the host operating system's message
inside the operation-specific `IOError`, so wording differs by platform.
`ReadWriteMode` does not request file creation on any platform. Files are opened
in binary host mode and Solveig performs UTF-8 decoding and newline handling,
which keeps text and byte writes consistent across Unix and Windows. Paths
otherwise follow host syntax and filesystem semantics.
