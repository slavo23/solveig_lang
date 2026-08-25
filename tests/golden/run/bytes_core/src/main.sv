import std.bytes(*)

printByte : Maybe Int -> Task ()
printByte maybeByte =
    case maybeByte of
        Just byte => printLine (show byte)
        | Nothing => printLine "missing"

printText : Either Utf8Error String -> Task ()
printText result =
    case result of
        Right text => printLine text
        | Left error => printLine "invalid UTF-8"

main : Task ()
main = do
    bytes <- pureIO (stringToBytes "hello")
    printLine (show (bytesLength bytes))
    printByte (bytesIndex bytes 1)
    printByte (bytesIndex bytes 99)
    printText
        (bytesToString
            (bytesAppend
                (bytesSlice bytes 1 3)
                (stringToBytes "!")))
