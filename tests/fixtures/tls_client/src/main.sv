import std.bytes(*)
import std.env(getArgs)
import std.fs(FsError(..))
import std.net(Port, portFromInt)
import std.path(Path, pathFromString)
import std.stream(fold)
import std.task(withThread)
import std.text(charToCodePoint, stringToChars)
import std.tls(*)

digitValue : Char -> Int
digitValue character = charToCodePoint character - charToCodePoint '0'

decimal : [Char] -> Int -> Int
decimal [] value = value
decimal (character :: rest) value = decimal rest (value * 10 + digitValue character)

fixturePort : [String] -> Port
fixturePort (spelling :: _) =
    case portFromInt (decimal (stringToChars spelling) 0) of
        Right port => port
        | Left _ => error "invalid fixture port"
fixturePort [] = error "missing fixture port"

certificatePath : [String] -> Path
certificatePath (_ :: spelling :: _) =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error (show pathError)
certificatePath _ = error "missing fixture certificate path"

fsToIO : FsError -> IOError
fsToIO (FsIOError _ ioError) = ioError
fsToIO error = IOError InvalidArgument "filesystem" (show error)

optionsFor : Bytes -> TlsOptions
optionsFor certificate =
    withTlsServerName "example.com" . withTrustedCertificatesPem certificate $ defaultTlsOptions

collectReply : forall scope. TlsConnection scope -> IO TlsError Bytes
collectReply connection = do
    sendTlsBytes connection (stringToBytes "ping")
    fold bytesAppend (stringToBytes "") (tlsBytes connection 2)

showReply : Bytes -> String
showReply bytes =
    show (bytesLength bytes)
        <> ":"
        <> show (bytesIndex bytes 0)
        <> ":"
        <> show (bytesIndex bytes 1)
        <> ":"
        <> show (bytesIndex bytes 3)

showTlsFailure : TlsError -> String
showTlsFailure (InvalidTlsChunkSize _) = "chunk"
showTlsFailure (TlsNetworkError _) = "network"
showTlsFailure (TlsIOError _ ioError) =
    case ioErrorKind ioError of
        InvalidArgument => "configuration"
        | ProtocolError => "protocol"
        | ResourceVanished => "vanished"
        | _ => "host"

tlsToIO : TlsError -> IOError
tlsToIO failure = IOError InvalidArgument "tls" (show failure)

tlsTask : forall a. IO TlsError a -> Task a
tlsTask = mapError tlsToIO

captureTls : forall a. IO TlsError a -> IO TlsError (Either TlsError a)
captureTls action = catchError (fmap Right action) (fun failure => pureIO (Left failure))

showOutcome : String -> Either TlsError a -> String
showOutcome label (Left failure) = label <> ":" <> showTlsFailure failure
showOutcome label (Right _) = label <> ":unexpected"

expectFailure : String -> IO TlsError a -> Task ()
expectFailure label action = do
    outcome <- tlsTask $ captureTls action
    printLine $ showOutcome label outcome

idle : forall scope. TlsConnection scope -> IO TlsError ()
idle _ = pureIO ()

cancelledRead : forall scope. TlsConnection scope -> IO TlsError ()
cancelledRead connection = withThread (receiveTlsBytes connection 1) (fun _ => pureIO ())

cancelThenCollect : forall scope. TlsConnection scope -> IO TlsError Bytes
cancelThenCollect connection = do
    cancelledRead connection
    collectReply connection

main : Task ()
main = do
    arguments <- getArgs
    certificate <- mapError fsToIO (std.fs.readFileBytes (certificatePath arguments))
    let port = fixturePort arguments
    let options = optionsFor certificate
    reply <- tlsTask $ withTlsConnectionWith options "localhost" port collectReply
    printLine $ "reply:" <> showReply reply
    expectFailure
        "name"
        (withTlsConnectionWith
            (withTrustedCertificatesPem certificate defaultTlsOptions)
            "localhost"
            port
            idle)
    expectFailure
        "trust"
        (withTlsConnectionWith
            (withTrustedCertificatesPem (stringToBytes "") defaultTlsOptions)
            "localhost"
            port
            idle)
    finalReply <- tlsTask $ withTlsConnectionWith options "localhost" port cancelThenCollect
    printLine $ "cancel:" <> showReply finalReply
    expectFailure "truncated" $ withTlsConnectionWith options "localhost" port collectReply
    expectFailure "chunk" $
        withTlsConnectionWith
            options
            "localhost"
            port
            (fun connection => receiveTlsBytes connection 0)
