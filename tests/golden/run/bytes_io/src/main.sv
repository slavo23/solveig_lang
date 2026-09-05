import std.bytes(*)
import std.fs(FsError(..))
import std.io(*)
import std.path(Path, pathFromString)

fsToHost : FsError -> HostError
fsToHost (FsHostError _ hostError) = hostError
fsToHost error = HostError InvalidArgument "filesystem" (show error)

requiredPath : String -> Path
requiredPath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error (show pathError)

asTask : forall a. IO FsError a -> Task a
asTask = mapError fsToHost

showByte : Maybe Int -> String
;
showByte byte =
    case byte of
        Just value => show value
        | Nothing => "missing" ;

printBytes : Bytes -> Task ()
;
printBytes bytes = do
    printLine (show (bytesLength bytes))
    printLine (showByte (bytesIndex bytes 0))
    printLine (showByte (bytesIndex bytes 1))

reportNegative : HostError -> Task String
;
reportNegative ioError =
    if hostErrorOperation ioError == "handle read"
        then pureIO "negative-read"
        else pureIO "wrong-negative-error"
;

reportFileLimit : String -> HostError -> Task String
;
reportFileLimit expected ioError =
    if hostErrorOperation ioError == "readFileLimited"
        then pureIO expected
        else pureIO "wrong-file-limit-error"
;

readChunks : Handle -> Task ()
;
readChunks input = do
    first <- hGetBytes input 2
    second <- hGetBytes input 2
    third <- hGetBytes input 20
    eof <- hGetBytes input 1
    negative <-
        catchError
            (bindIO (hGetBytes input (0 - 1)) (fun bytes => pureIO "wrong-negative-success"))
            reportNegative
    printBytes first
    printBytes second
    printBytes third
    printLine (show (bytesLength eof))
    printLine negative

readMixed : Handle -> Task ()
;
readMixed input = do
    first <- hGetBytes input 1
    line <- hGetLine input
    final <- hGetBytes input 1
    printBytes first
    printLine line
    printBytes final

writeChunks : Bytes -> Handle -> Task ()
;
writeChunks payload output = do
    hSetBuffering output (BlockBuffering (Just 3))
    hPutStr output "A"
    hPutBytes output (bytesSlice payload 1 4)
    hFlush output

main : Task ()
;
main = do
    payload <- pureIO (stringToBytes "Aλ\nB")
    let generated = requiredPath "generated.bin"
    asTask (std.fs.writeFileBytes generated payload)
    whole <- asTask (std.fs.readFileBytes generated)
    printBytes whole
    limitedBytes <- asTask (std.fs.readFileBytesLimited generated 5)
    printBytes limitedBytes
    limitedText <- asTask (std.fs.readFileLimited generated 5)
    printLine (show (bytesLength (stringToBytes limitedText)))
    tooLarge <-
        catchError
            (bindIO
                (asTask (std.fs.readFileBytesLimited generated 4))
                (fun bytes => pureIO "wrong-file-limit-success"))
            (reportFileLimit "file-too-large")
    negativeLimit <-
        catchError
            (bindIO
                (asTask (std.fs.readFileLimited generated (0 - 1)))
                (fun text => pureIO "wrong-negative-limit-success"))
            (reportFileLimit "negative-file-limit")
    printLine tooLarge
    printLine negativeLimit
    read <- withFile "generated.bin" ReadMode readChunks
    mixed <- withFile "generated.bin" ReadMode readMixed
    wrote <- withFile "generated.bin" WriteMode (writeChunks payload)
    final <- asTask (std.fs.readFileBytes generated)
    printBytes final
