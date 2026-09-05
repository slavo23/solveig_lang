export (IOMode(..),
    BufferMode(..),
    stdin,
    stdout,
    stderr,
    openFile,
    hClose,
    hFlush,
    hGetLine,
    hGetContents,
    hGetBytes,
    hPutStr,
    hPutStrLn,
    hPutBytes,
    hIsEOF,
    hSetBuffering,
    withFile)

import prelude.prim as CorePrim(hFlush, hGetLine, hPutStr, hPutStrLn, stdin, stdout)
import std.prim as HostPrim(hClose,
    hGetBytes,
    hGetContents,
    hIsEOF,
    hPutBytes,
    hSetBuffering,
    openFile,
    stderr)
import std.task(bracket)

-- | How a file is opened.
data IOMode = ReadMode | WriteMode | AppendMode | ReadWriteMode

-- | A handle's buffering policy. Nothing selects the platform default block size.
data BufferMode = NoBuffering | LineBuffering | BlockBuffering (Maybe Int)

-- | The process standard-input handle.
stdin : Handle
stdin = CorePrim.stdin

-- | The process standard-output handle.
stdout : Handle
stdout = CorePrim.stdout

-- | The process standard-error handle.
stderr : Handle
stderr = HostPrim.stderr

-- | Opens a file handle. The handle must be closed explicitly.
openFile : String -> IOMode -> Task Handle
openFile path mode = HostPrim.openFile path mode

-- | Closes a handle, flushing pending output first.
hClose : Handle -> Task ()
hClose handle = HostPrim.hClose handle

-- | Flushes pending output from a handle.
hFlush : Handle -> Task ()
hFlush handle = CorePrim.hFlush handle

-- | Reads one line without its line terminator.
hGetLine : Handle -> Task String
hGetLine handle = CorePrim.hGetLine handle

-- | Strictly reads all remaining text from a handle.
hGetContents : Handle -> Task String
hGetContents handle = HostPrim.hGetContents handle

-- | Reads at most the requested number of arbitrary bytes, returning empty at EOF.
hGetBytes : Handle -> Int -> Task Bytes
hGetBytes handle maximum = HostPrim.hGetBytes handle maximum

-- | Writes text to a handle.
hPutStr : Handle -> String -> Task ()
hPutStr handle text = CorePrim.hPutStr handle text

-- | Writes text followed by a newline to a handle.
hPutStrLn : Handle -> String -> Task ()
hPutStrLn handle text = CorePrim.hPutStrLn handle text

-- | Writes arbitrary bytes to a handle.
hPutBytes : Handle -> Bytes -> Task ()
hPutBytes handle bytes = HostPrim.hPutBytes handle bytes

-- | Reports whether a readable handle is at end of input.
hIsEOF : Handle -> Task Bool
hIsEOF handle = HostPrim.hIsEOF handle

-- | Changes a handle's buffering policy, flushing pending output first.
hSetBuffering : Handle -> BufferMode -> Task ()
hSetBuffering handle mode = HostPrim.hSetBuffering handle mode

-- | Opens a file and closes it after use, including errors and interruption.
withFile : forall a. String -> IOMode -> (Handle -> Task a) -> Task a
withFile path mode use = bracket (openFile path mode) hClose use
