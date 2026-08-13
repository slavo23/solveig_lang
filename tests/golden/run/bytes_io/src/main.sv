showByte : Maybe Int -> String;
showByte byte =
    case byte of
        Just value => show value
        | Nothing => "missing";

printBytes : Bytes -> Task ();
printBytes bytes = do
    printLine (show (bytesLength bytes))
    printLine (showByte (bytesIndex bytes 0))
    printLine (showByte (bytesIndex bytes 1));

reportNegative : IOError -> Task String;
reportNegative ioError =
    case ioError of
        ReadHandleError message => pureIO "negative-read"
        | _ => pureIO "wrong-negative-error";

readChunks : Handle -> Task ();
readChunks input = do
    first <- hGetBytes input 2
    second <- hGetBytes input 2
    third <- hGetBytes input 20
    eof <- hGetBytes input 1
    negative <- catchError
        (bindIO (hGetBytes input (0 - 1)) (fun bytes => pureIO "wrong-negative-success"))
        reportNegative
    printBytes first
    printBytes second
    printBytes third
    printLine (show (bytesLength eof))
    printLine negative;

readMixed : Handle -> Task ();
readMixed input = do
    first <- hGetBytes input 1
    line <- hGetLine input
    final <- hGetBytes input 1
    printBytes first
    printLine line
    printBytes final;

writeChunks : Bytes -> Handle -> Task ();
writeChunks payload output = do
    hSetBuffering output (BlockBuffering (Just 3))
    hPutStr output "A"
    hPutBytes output (bytesSlice payload 1 4)
    hFlush output;

main : Task ();
main = do
    payload <- pureIO (stringToBytes "Aλ\nB")
    writeFileBytes "generated.bin" payload
    whole <- readFileBytes "generated.bin"
    printBytes whole
    read <- withFile "generated.bin" ReadMode readChunks
    mixed <- withFile "generated.bin" ReadMode readMixed
    wrote <- withFile "generated.bin" WriteMode (writeChunks payload)
    final <- readFileBytes "generated.bin"
    printBytes final;
