export (
    FsError(..),
    readFile,
    readFileLimited,
    readFileBytes,
    readFileBytesLimited,
    writeFile,
    writeFileAtomic,
    writeFileBytes,
    writeFileBytesAtomic,
    appendFile,
    appendFileBytes,
    withFileByteStream,
    withFileTextStream,
    withFileLineStream,
    getCurrentDirectory,
    setCurrentDirectory,
    doesFileExist,
    doesDirectoryExist,
    listDirectory,
    createDirectory,
    removeFile,
    renameFile,
    getFileSize,
    getModificationTime
)

import std.prim(
    appendFile,
    createDirectory,
    doesDirectoryExist,
    doesFileExist,
    getCurrentDirectory,
    getFileSize,
    getModificationTime,
    listDirectory,
    readFileBytes,
    readFileBytesLimited,
    removeFile,
    renameFile,
    setCurrentDirectory,
    writeFile,
    writeFileAtomic,
    writeFileBytes,
    writeFileBytesAtomic
)
import std.bytes(*)
import std.io(IOMode(..), hClose, hGetBytes, hPutBytes, openFile)
import std.path(Path, currentPath, pathFromHost, pathToString)
import std.stream(Stream, unfold)
import std.task(bracket)
import std.time(SystemTime, systemTimeFromNanoseconds)

-- | A filesystem failure with the path context known by the public operation.
data FsError = FsIOError Path IOError | FsInvalidUtf8 Path Utf8Error

instance Eq FsError where
    (==) (FsIOError leftPath leftIOError) (FsIOError rightPath rightIOError) =
        and (leftPath == rightPath) (leftIOError == rightIOError)
    (==) (FsInvalidUtf8 leftPath leftUtf8) (FsInvalidUtf8 rightPath rightUtf8) =
        and (leftPath == rightPath) (leftUtf8 == rightUtf8)
    (==) _ _ = False

instance Show FsError where
    show (FsIOError path ioError) = "filesystem " <> show path <> ": " <> show ioError
    show (FsInvalidUtf8 path utf8Error) = "filesystem " <> show path <> ": " <> show utf8Error

atPath : forall a. Path -> Task a -> IO FsError a
atPath path operation = mapError (FsIOError path) operation

-- | Strictly reads an entire trusted file as UTF-8 text.
-- |
-- | This convenient operation has no library size bound. Use readFileLimited
-- | for untrusted input or a scoped stream for input that may not fit memory.
readFile : Path -> IO FsError String
readFile path = bindIO (readFileBytes path) (decodeFile path 0)

-- | Strictly reads at most maximum bytes and decodes them as UTF-8 text.
-- |
-- | The host reads at most one byte beyond the non-negative limit, so the size
-- | check cannot race with a growing file.
readFileLimited : Path -> Int -> IO FsError String
readFileLimited path maximum = bindIO (readFileBytesLimited path maximum) (decodeFile path 0)

-- | Strictly reads an entire trusted file as arbitrary bytes.
readFileBytes : Path -> IO FsError Bytes
readFileBytes path = atPath path (std.prim.readFileBytes (pathToString path))

-- | Strictly reads arbitrary bytes and fails when the file exceeds maximum.
readFileBytesLimited : Path -> Int -> IO FsError Bytes
readFileBytesLimited path maximum =
    atPath path (std.prim.readFileBytesLimited (pathToString path) maximum)

-- | Writes UTF-8 text, creating the file or replacing its previous contents.
writeFile : Path -> String -> IO FsError ()
writeFile path contents = atPath path (std.prim.writeFile (pathToString path) contents)

-- | Atomically replaces a file with UTF-8 text on hosts that support replace-by-rename.
-- |
-- | The complete replacement is written and synced beside the destination
-- | before one host rename makes it visible. Failure removes the temporary file
-- | and leaves the previous destination untouched. Hosts unable to replace an
-- | existing destination atomically return a typed I/O failure.
writeFileAtomic : Path -> String -> IO FsError ()
writeFileAtomic path contents = atPath path (std.prim.writeFileAtomic (pathToString path) contents)

-- | Writes arbitrary bytes, creating the file or replacing its contents.
writeFileBytes : Path -> Bytes -> IO FsError ()
writeFileBytes path contents = atPath path (std.prim.writeFileBytes (pathToString path) contents)

-- | Atomically replaces a file with arbitrary bytes.
-- |
-- | Its visibility, cleanup, and host-support contract matches writeFileAtomic.
writeFileBytesAtomic : Path -> Bytes -> IO FsError ()
writeFileBytesAtomic path contents =
    atPath path (std.prim.writeFileBytesAtomic (pathToString path) contents)

-- | Appends UTF-8 text to a file, creating it when it does not exist.
appendFile : Path -> String -> IO FsError ()
appendFile path contents = atPath path (std.prim.appendFile (pathToString path) contents)

-- | Appends arbitrary bytes to a file, creating it when it does not exist.
appendFileBytes : Path -> Bytes -> IO FsError ()
appendFileBytes path contents =
    bracket
        (atPath path (openFile (pathToString path) AppendMode))
        (atPath path . hClose)
        (fun handle => atPath path (hPutBytes handle contents))

-- | Opens a file as a scoped stream of non-empty byte chunks.
-- |
-- | The positive chunk size bounds every pull and no read-ahead occurs. The
-- | callback's universal scope prevents the stream from escaping its handle.
-- | Success, failure, defects, and interruption all close the file.
withFileByteStream :
    forall result. Int ->
    Path -> (forall scope. Stream scope FsError Bytes -> IO FsError result) -> IO FsError result
withFileByteStream chunkSize path use =
    if chunkSize <= 0
        then invalidChunkSize path "withFileByteStream" chunkSize
        else withReadHandle path (fun handle => use (fileByteStream path chunkSize handle))

-- | Opens a file as a scoped stream of non-empty UTF-8 text chunks.
-- |
-- | Byte chunk boundaries never become character boundaries: an incomplete
-- | UTF-8 scalar is retained for the next pull. Invalid input reports its
-- | absolute byte offset through FsInvalidUtf8.
withFileTextStream :
    forall result. Int ->
    Path -> (forall scope. Stream scope FsError String -> IO FsError result) -> IO FsError result
withFileTextStream chunkSize path use =
    if chunkSize <= 0
        then invalidChunkSize path "withFileTextStream" chunkSize
        else withReadHandle path (fun handle => use (fileTextStream path chunkSize handle))

-- | Opens a file as a scoped stream of UTF-8 lines without terminators.
-- |
-- | Both LF and CRLF are accepted. Blank lines are retained. The chunk size
-- | bounds each host read, while one unusually long line may itself require
-- | proportional memory. Invalid UTF-8 retains its absolute byte offset.
withFileLineStream :
    forall result. Int ->
    Path -> (forall scope. Stream scope FsError String -> IO FsError result) -> IO FsError result
withFileLineStream chunkSize path use =
    if chunkSize <= 0
        then invalidChunkSize path "withFileLineStream" chunkSize
        else withReadHandle path (fun handle => use (fileLineStream path chunkSize handle))

withReadHandle : forall result. Path -> (Handle -> IO FsError result) -> IO FsError result
withReadHandle path =
    bracket (atPath path (openFile (pathToString path) ReadMode)) (atPath path . hClose)

fileByteStream : Path -> Int -> Handle -> Stream () FsError Bytes
fileByteStream path chunkSize handle = unfold (fileByteStep path chunkSize handle) ()

fileByteStep : Path -> Int -> Handle -> () -> IO FsError (Maybe (Bytes, ()))
fileByteStep path chunkSize handle _ = do
    bytes <- atPath path (hGetBytes handle chunkSize)
    if bytesLength bytes == 0 then pureIO Nothing else pureIO (Just (bytes, ()))

fileTextStream : Path -> Int -> Handle -> Stream () FsError String
fileTextStream path chunkSize handle =
    unfold (fileTextStep path chunkSize) (handle, stringToBytes "", 0)

fileTextStep :
    Path -> Int -> (Handle, Bytes, Int) -> IO FsError (Maybe (String, (Handle, Bytes, Int)))
fileTextStep path chunkSize (handle, buffered, offset) =
    case bytesUtf8ValidPrefix buffered False of
        Left utf8Error => failIO (absoluteUtf8Error path offset utf8Error)
        | Right prefixLength =>
            if prefixLength > 0
                then
                    pureIO
                        (Just
                            (
                                decodeUtf8Prefix buffered prefixLength,
                                (
                                    handle,
                                    bytesSlice
                                        buffered
                                        prefixLength
                                        (bytesLength buffered - prefixLength),
                                    offset + prefixLength
                                )
                            ))
                else do
                        bytes <- atPath path (hGetBytes handle chunkSize)
                        if bytesLength bytes == 0
                            then finishText path handle buffered offset
                            else
                                fileTextStep
                                    path
                                    chunkSize
                                    (handle, bytesAppend buffered bytes, offset)

finishText : Path -> Handle -> Bytes -> Int -> IO FsError (Maybe (String, (Handle, Bytes, Int)))
finishText path handle buffered offset =
    case bytesUtf8ValidPrefix buffered True of
        Left utf8Error => failIO (absoluteUtf8Error path offset utf8Error)
        | Right prefixLength =>
            if prefixLength == 0
                then pureIO Nothing
                else
                    pureIO
                        (Just
                            (
                                decodeUtf8Prefix buffered prefixLength,
                                (handle, stringToBytes "", offset + prefixLength)
                            ))

fileLineStream : Path -> Int -> Handle -> Stream () FsError String
fileLineStream path chunkSize handle =
    unfold (fileLineStep path chunkSize) (handle, emptyBytesBuilder, 0, stringToBytes "", 0, False)

fileLineStep :
    Path ->
    Int ->
    (Handle, BytesBuilder, Int, Bytes, Int, Bool) ->
    IO FsError (Maybe (String, (Handle, BytesBuilder, Int, Bytes, Int, Bool)))
fileLineStep path chunkSize (handle, builder, bufferedLength, pending, offset, endOfInput) =
    let newline =
        bytesFindAny pending 0 (stringToBytes "\n")
    in if newline < bytesLength pending
        then
            emitLine
                path
                handle
                builder
                bufferedLength
                pending
                offset
                endOfInput
                newline
                (newline + 1)
        else if endOfInput
                then
                    if bufferedLength + bytesLength pending == 0
                        then pureIO Nothing
                        else
                            emitLine
                                path
                                handle
                                builder
                                bufferedLength
                                pending
                                offset
                                True
                                (bytesLength pending)
                                (bytesLength pending)
                else do
                        bytes <- atPath path (hGetBytes handle chunkSize)
                        fileLineStep
                            path
                            chunkSize
                            (
                                handle,
                                builder <> bytesBuilder pending,
                                bufferedLength + bytesLength pending,
                                bytes,
                                offset,
                                bytesLength bytes == 0
                            )

emitLine :
    Path ->
    Handle ->
    BytesBuilder ->
    Int ->
    Bytes ->
    Int ->
    Bool -> Int -> Int -> IO FsError (Maybe (String, (Handle, BytesBuilder, Int, Bytes, Int, Bool)))
emitLine path handle builder bufferedLength pending offset endOfInput lineEnd consumed =
    let lineBytes =
        buildBytes (builder <> bytesBuilder (bytesSlice pending 0 lineEnd))
    in let contentLength =
        stripCarriageReturn lineBytes (bytesLength lineBytes)
    in case bytesSliceToString lineBytes 0 contentLength of
        Left utf8Error => failIO (absoluteUtf8Error path offset utf8Error)
        | Right line =>
            pureIO
                (Just
                    (
                        line,
                        (
                            handle,
                            emptyBytesBuilder,
                            0,
                            bytesSlice pending consumed (bytesLength pending - consumed),
                            offset + bufferedLength + consumed,
                            endOfInput
                        )
                    ))

stripCarriageReturn : Bytes -> Int -> Int
stripCarriageReturn bytes lineEnd =
    if lineEnd == 0
        then 0
        else
            case bytesIndex bytes (lineEnd - 1) of
                Just 13 => lineEnd - 1
                | _ => lineEnd

decodeFile : Path -> Int -> Bytes -> IO FsError String
decodeFile path offset bytes =
    case bytesToString bytes of
        Left utf8Error => failIO (absoluteUtf8Error path offset utf8Error)
        | Right text => pureIO text

decodeUtf8Prefix : Bytes -> Int -> String
decodeUtf8Prefix bytes length =
    case bytesSliceToString bytes 0 length of
        Right text => text
        | Left _ => error "std.fs internal UTF-8 prefix invariant failed"

absoluteUtf8Error : Path -> Int -> Utf8Error -> FsError
absoluteUtf8Error path base (InvalidUtf8 offset) = FsInvalidUtf8 path (InvalidUtf8 (base + offset))

invalidChunkSize : forall a. Path -> String -> Int -> IO FsError a
invalidChunkSize path operation chunkSize =
    failIO
        (FsIOError
            path
            (IOError
                InvalidArgument
                operation
                ("chunk size must be positive, got " <> show chunkSize)))

-- | Returns the runtime's current working directory.
getCurrentDirectory : IO FsError Path
getCurrentDirectory = fmap pathFromHost (atPath currentPath std.prim.getCurrentDirectory)

-- | Changes the runtime's working directory after verifying it exists.
setCurrentDirectory : Path -> IO FsError ()
setCurrentDirectory path = atPath path (std.prim.setCurrentDirectory (pathToString path))

-- | Reports whether a path names a regular file.
doesFileExist : Path -> IO FsError Bool
doesFileExist path = atPath path (std.prim.doesFileExist (pathToString path))

-- | Reports whether a path names a directory.
doesDirectoryExist : Path -> IO FsError Bool
doesDirectoryExist path = atPath path (std.prim.doesDirectoryExist (pathToString path))

-- | Lists a directory's immediate entries as paths in lexical order.
listDirectory : Path -> IO FsError [Path]
listDirectory path =
    fmap (map pathFromHost) (atPath path (std.prim.listDirectory (pathToString path)))

-- | Creates one directory; its parent must already exist.
createDirectory : Path -> IO FsError ()
createDirectory path = atPath path (std.prim.createDirectory (pathToString path))

-- | Removes one file.
removeFile : Path -> IO FsError ()
removeFile path = atPath path (std.prim.removeFile (pathToString path))

-- | Renames a file or directory.
renameFile : Path -> Path -> IO FsError ()
renameFile source destination =
    atPath source (std.prim.renameFile (pathToString source) (pathToString destination))

-- | Returns a file's size in bytes.
getFileSize : Path -> IO FsError Int
getFileSize path = atPath path (std.prim.getFileSize (pathToString path))

-- | Returns a path's wall-clock modification time.
getModificationTime : Path -> IO FsError SystemTime
getModificationTime path =
    fmap systemTimeFromNanoseconds (atPath path (std.prim.getModificationTime (pathToString path)))
