import std.bytes(bytesAppend, bytesLength, bytesSlice, stringToBytes)
import std.fs(FsError(..))
import std.list(length)
import std.path(*)
import std.stream
import std.text(codePointToString)

fixturePath : String -> Path
fixturePath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error ("invalid fixture path: " <> show pathError)

fsToHost : FsError -> HostError
fsToHost (FsHostError _ hostError) = hostError
fsToHost (FsInvalidUtf8 path utf8Error) =
    HostError InvalidArgument "filesystemUtf8" (show path <> ": " <> show utf8Error)

asTask : forall a. IO FsError a -> Task a
asTask = mapError fsToHost

ensure : String -> Bool -> Task ()
ensure label condition =
    if condition then printLine ("ok: " <> label) else error ("failed: " <> label)

checkPaths : Task ()
checkPaths =
    let base =
        fixturePath "data"
    in let joined =
        base </> fixturePath "items/file.txt"
    in let reduced =
        normalise (fixturePath "data/./items/../file.txt")
    in ensure
        "platform path algebra"
        (and
            (joined == fixturePath "data/items/file.txt")
            (and
                (reduced == fixturePath "data/file.txt")
                (and
                    (parent joined == Just (fixturePath "data/items"))
                    (and
                        (fileName joined == Just "file.txt")
                        (and
                            (extension joined == Just "txt")
                            (and (isRelative joined) (not (isAbsolute joined))))))))

appendText : String -> String -> String
appendText = (<>)

collectText : forall scope. std.stream.Stream scope FsError String -> IO FsError String
collectText = std.stream.fold appendText ""

collectLines :
    forall scope. std.stream.Stream scope FsError String ->
    IO FsError (Either std.stream.StreamError [String])
collectLines = std.stream.toListLimited 8

countBytes : Int -> Bytes -> Int
countBytes total bytes = total + bytesLength bytes

collectByteCount : forall scope. std.stream.Stream scope FsError Bytes -> IO FsError Int
collectByteCount = std.stream.fold countBytes 0

isInvalidAt : Int -> FsError -> Bool
isInvalidAt expected (FsInvalidUtf8 _ (InvalidUtf8 actual)) = expected == actual
isInvalidAt _ _ = False

observeInvalid : forall a. Int -> IO FsError a -> Task Bool
observeInvalid expected action =
    catchError
        (bindIO action (fun ignored => pureIO False))
        (fun fsError => pureIO (isInvalidAt expected fsError))

checkFiles : Task ()
checkFiles = do
    let textPath = fixturePath "generated-filesystem-text.txt"
    let invalidPath = fixturePath "generated-filesystem-invalid.bin"
    let carriageReturn =
        case codePointToString 13 of
            Just character => character
            | Nothing => error "carriage return must be a Unicode scalar"
    let contents = "Aé\nB" <> carriageReturn <> "\n\nlast"
    asTask (std.fs.writeFile textPath contents)
    asTask (std.fs.writeFileAtomic textPath ("old" <> contents))
    asTask (std.fs.writeFileBytesAtomic textPath (stringToBytes contents))
    strict <- asTask (std.fs.readFile textPath)
    limited <- asTask (std.fs.readFileLimited textPath 64)
    streamed <- asTask (std.fs.withFileTextStream 2 textPath collectText)
    lines <- asTask (std.fs.withFileLineStream 2 textPath collectLines)
    byteCount <- asTask (std.fs.withFileByteStream 3 textPath collectByteCount)
    let incomplete = bytesSlice (stringToBytes "é") 0 1
    let malformed = bytesAppend (stringToBytes "ok") incomplete
    asTask (std.fs.writeFileBytes invalidPath malformed)
    strictInvalid <- observeInvalid 2 (std.fs.readFile invalidPath)
    streamInvalid <- observeInvalid 2 (std.fs.withFileTextStream 2 invalidPath collectText)
    ensure
        "strict and scoped filesystem text"
        (and
            (strict == contents)
            (and
                (limited == contents)
                (and
                    (streamed == contents)
                    (and
                        (lines == Right ["Aé", "B", "", "last"])
                        (and
                            (byteCount == bytesLength (stringToBytes contents))
                            (and strictInvalid streamInvalid))))))
    asTask (std.fs.removeFile invalidPath)
    asTask (std.fs.removeFile textPath)

main : Task ()
main = do
    checkPaths
    checkFiles
