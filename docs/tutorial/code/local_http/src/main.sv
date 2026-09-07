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
    "HTTP/1.1 200 OK\u{d}\n" <>
        "Content-Type: application/json\u{d}\n" <>
        "Content-Length: " <>
        show (bytesLength (stringToBytes fixtureBody)) <>
        "\u{d}\n" <>
        "Connection: close\u{d}\n\u{d}\n" <>
        fixtureBody

netToHost : NetError -> HostError
netToHost (NetHostError _ hostError) = hostError
netToHost error = HostError InvalidArgument "network" (show error)

hostToNet : HostError -> NetError
hostToNet hostError = NetHostError "fixture" hostError

reply : forall scope. TcpConnection scope -> IO NetError ()
reply connection = do
    _ <- receiveTcpBytes connection 4096
    sendTcpBytes connection (stringToBytes fixtureResponse)

serveBatch : forall scope. TcpListener scope -> Int -> IO NetError ()
serveBatch _ 0 = pureIO ()
serveBatch listener remaining = do
    withAcceptedTcpConnection listener reply
    serveBatch listener (remaining - 1)

httpAsHostError : HttpError -> HostError
httpAsHostError (HttpHostError hostError) = hostError
httpAsHostError error = HostError InvalidArgument "HTTP" (show error)

messageDecoder : Decoder () String
messageDecoder = recordDecoder $ requiredField "message" stringDecoder

decodeMessage : Bytes -> Either HostError String
decodeMessage bytes =
    foldReply
        (fun _ message _ => Right message)
        (fun _ _ parseError =>
            Left $ HostError InvalidArgument "decode message response" $ show parseError
        )
        (fun _ _ _ => Left $ HostError InvalidArgument "decode message response" "incomplete JSON")
        (decodeJson defaultJsonLimits messageDecoder bytes)

fetchMessage : String -> Task String
fetchMessage url = do
    response <-
        mapError httpAsHostError $
            getWith
                (withHeader "Accept" "application/json" $
                    withQuery [("language", "solveig")] defaultOptions)
                url
    successful <-
        case ensureSuccess response of
            Left error => failIO $ httpAsHostError error
            | Right value => pureIO value
    case decodeMessage $ std.http.responseBody successful of
        Left error => failIO error
        | Right message => pureIO message

runClients : Port -> Task [String]
runClients port =
    let url =
        "http://127.0.0.1:" <> show port <> "/message"
    in mapError concurrentHostError $ mapConcurrentlyBoundedOrdered 3 fetchMessage [url, url, url]

runFixture : forall scope. TcpListener scope -> Port -> IO NetError ()
runFixture listener port =
    withThread
        (serveBatch listener 3)
        (fun _ =>
            do
                bodies <- mapError hostToNet (runClients port)
                mapError hostToNet (printLine ("responses: " <> show (length bodies)))
                mapError hostToNet $
                    printLine
                        (case bodies of
                            first :: _ => first
                            | [] => "none"
                        )
        )

main : Task ()
main = mapError netToHost (withTcpListener anyPort runFixture)
