import prelude.prim
import std.task(bracket)

-- | How a file is opened.
data IOMode =
    ReadMode
  | WriteMode
  | AppendMode
  | ReadWriteMode

-- | A handle's buffering policy. Nothing selects the platform default block size.
data BufferMode =
    NoBuffering
  | LineBuffering
  | BlockBuffering (Maybe Int)

-- | The process standard-input handle.
stdin : Handle
stdin = prelude.prim.stdin

-- | The process standard-output handle.
stdout : Handle
stdout = prelude.prim.stdout

-- | The process standard-error handle.
stderr : Handle
stderr = prelude.prim.stderr

-- | Opens a file handle. The handle must be closed explicitly.
openFile : String -> IOMode -> Task Handle
openFile path mode = prelude.prim.openFile path mode

-- | Closes a handle, flushing pending output first.
hClose : Handle -> Task ()
hClose handle = prelude.prim.hClose handle

-- | Flushes pending output from a handle.
hFlush : Handle -> Task ()
hFlush handle = prelude.prim.hFlush handle

-- | Reads one line without its line terminator.
hGetLine : Handle -> Task String
hGetLine handle = prelude.prim.hGetLine handle

-- | Strictly reads all remaining text from a handle.
hGetContents : Handle -> Task String
hGetContents handle = prelude.prim.hGetContents handle

-- | Reads at most the requested number of arbitrary bytes, returning empty at EOF.
hGetBytes : Handle -> Int -> Task Bytes
hGetBytes handle maximum = prelude.prim.hGetBytes handle maximum

-- | Writes text to a handle.
hPutStr : Handle -> String -> Task ()
hPutStr handle text = prelude.prim.hPutStr handle text

-- | Writes text followed by a newline to a handle.
hPutStrLn : Handle -> String -> Task ()
hPutStrLn handle text = prelude.prim.hPutStrLn handle text

-- | Writes arbitrary bytes to a handle.
hPutBytes : Handle -> Bytes -> Task ()
hPutBytes handle bytes = prelude.prim.hPutBytes handle bytes

-- | Reports whether a readable handle is at end of input.
hIsEOF : Handle -> Task Bool
hIsEOF handle = prelude.prim.hIsEOF handle

-- | Changes a handle's buffering policy, flushing pending output first.
hSetBuffering : Handle -> BufferMode -> Task ()
hSetBuffering handle mode = prelude.prim.hSetBuffering handle mode

-- | Opens a file and closes it after use, including errors and interruption.
withFile : forall a. String -> IOMode -> (Handle -> Task a) -> Task a
withFile path mode use = bracket (openFile path mode) hClose use

-- | Strictly reads an entire file as text without a library size limit.
readFile : String -> Task String
readFile path = prelude.prim.readFile path

-- | Strictly reads at most maximum bytes as text and fails if the file is larger.
readFileLimited : String -> Int -> Task String
readFileLimited path maximum = prelude.prim.readFileLimited path maximum

-- | Strictly reads an entire file as arbitrary bytes without a library size limit.
readFileBytes : String -> Task Bytes
readFileBytes path = prelude.prim.readFileBytes path

-- | Strictly reads at most maximum arbitrary bytes and fails if the file is larger.
readFileBytesLimited : String -> Int -> Task Bytes
readFileBytesLimited path maximum = prelude.prim.readFileBytesLimited path maximum

-- | Writes text to a file, replacing its previous contents.
writeFile : String -> String -> Task ()
writeFile path contents = prelude.prim.writeFile path contents

-- | Writes arbitrary bytes to a file, replacing its previous contents.
writeFileBytes : String -> Bytes -> Task ()
writeFileBytes path contents = prelude.prim.writeFileBytes path contents

-- | Appends text to the end of a file.
appendFile : String -> String -> Task ()
appendFile path contents = prelude.prim.appendFile path contents
