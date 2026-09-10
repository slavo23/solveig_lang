import std.bytes(bytesLength)
import std.random(*)
import std.text(intToString)

printSecure : String -> IO SecureRandomError ()
printSecure text = mapError SecureRandomHostError (printLine text)

printLength : String -> Bytes -> IO SecureRandomError ()
printLength label bytes = printSecure (label <> intToString (bytesLength bytes))

invalidCount : IO SecureRandomError ()
invalidCount =
    catchError
        (bindIO
            (secureRandomBytes (-1))
            (fun bytes => printSecure "unexpected negative count success"))
        (fun randomError =>
            case randomError of
                InvalidSecureRandomByteCount count =>
                    printSecure ("invalid count: " <> intToString count)
                | SecureRandomHostError hostError =>
                    printSecure ("unexpected host failure: " <> show hostError)
        )

main : IO SecureRandomError ()
main = do
    bytes <- secureRandomBytes 32
    empty <- secureRandomBytes 0
    printLength "secure bytes: " bytes
    printLength "empty bytes: " empty
    invalidCount
