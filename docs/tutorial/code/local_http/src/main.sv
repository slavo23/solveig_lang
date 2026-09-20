import std.bytes(*)
import std.http(*)
import std.json(Decoder, decodeJson, defaultJsonLimits, recordDecoder, requiredField, stringDecoder)
import std.list(length)
import std.net(*)
import std.parser(foldReply)
import std.task(*)

fixtureBody : String
fixtureBody = "{\"message\":\"hello from Solveig\"}"

fixtureResponse : String
fixtureResponse =
    "HTTP/1.1 200 OK\u{d}\n"
        <> "Content-Type: application/json\u{d}\n"
        <> "Content-Length: "
        <> show (bytesLength (stringToBytes fixtureBody))
        <> "\u{d}\n"
        <> "Connection: close\u{d}\n\u{d}\n"
        <> fixtureBody

netToIO : NetError -> IOError
netToIO (NetIOError _ ioError) = ioError
netToIO error = IOError InvalidArgument "network" (show error)

ioToNet : IOError -> NetError
ioToNet ioError = NetIOError "fixture" ioError

reply : forall scope. TcpConnection scope -> IO NetError ()
reply connection = do
    _ <- receiveTcpBytes connection 4096
    sendTcpBytes connection (stringToBytes fixtureResponse)

serveBatch : forall scope. TcpListener scope -> Int -> IO NetError ()
serveBatch _ 0 = pureIO ()
serveBatch listener remaining = do
    withAcceptedTcpConnection listener reply
    serveBatch listener (remaining - 1)

httpAsIOError : HttpError -> IOError
httpAsIOError (HttpIOError ioError) = ioError
httpAsIOError error = IOError InvalidArgument "HTTP" (show error)

messageDecoder : Decoder () String
messageDecoder = recordDecoder $ requiredField "message" stringDecoder

decodeMessage : Bytes -> Either IOError String
decodeMessage bytes =
    foldReply
        (fun _ message _ => Right message)
        (fun _ _ parseError =>
            Left $ IOError InvalidArgument "decode message response" $ show parseError
        )
        (fun _ _ _ => Left $ IOError InvalidArgument "decode message response" "incomplete JSON")
        (decodeJson defaultJsonLimits messageDecoder bytes)

fetchMessage : String -> Task String
fetchMessage url = do
    response <-
        mapError httpAsIOError $
            getWith
                (withHeader "Accept" "application/json" $
                    withQuery [("language", "solveig")] defaultOptions)
                url
    successful <-
        case ensureSuccess response of
            Left error => failIO $ httpAsIOError error
            | Right value => pureIO value
    case decodeMessage $ std.http.responseBody successful of
        Left error => failIO error
        | Right message => pureIO message

runClients : Port -> Task [String]
runClients port =
    let url =
        "http://127.0.0.1:" <> show port <> "/message"
    in mapError concurrentIOError $ mapConcurrentlyBoundedOrdered 3 fetchMessage [url, url, url]

runFixture : forall scope. TcpListener scope -> Port -> IO NetError ()
runFixture listener port =
    withThread
        (serveBatch listener 3)
        (fun _ => do
            bodies <- mapError ioToNet (runClients port)
            mapError ioToNet (printLine ("responses: " <> show (length bodies)))
            mapError ioToNet $
                printLine
                    (case bodies of
                        first :: _ => first
                        | [] => "none"
                    )
        )

main : Task ()
main = mapError netToIO (withTcpListener anyPort runFixture)
