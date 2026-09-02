import std.bytes(*)
import std.http(*)
import std.list(length)
import std.net(*)
import std.task(*)

responseBody : String
responseBody = "{\"message\":\"hello from Solveig\"}"

responseText : String
responseText =
    "HTTP/1.1 200 OK\u{d}\n" <>
        "Content-Type: application/json\u{d}\n" <>
        "Content-Length: " <>
        show (bytesLength (stringToBytes responseBody)) <>
        "\u{d}\n" <>
        "Connection: close\u{d}\n\u{d}\n" <>
        responseBody

netToHost : NetError -> HostError
netToHost (NetHostError _ hostError) = hostError
netToHost error = HostError InvalidArgument "network" (show error)

hostToNet : HostError -> NetError
hostToNet hostError = NetHostError "fixture" hostError

reply : forall scope. TcpConnection scope -> IO NetError ()
reply connection = do
    _ <- receiveTcpBytes connection 4096
    sendTcpBytes connection (stringToBytes responseText)

serveBatch : forall scope. TcpListener scope -> Int -> IO NetError ()
serveBatch _ 0 = pureIO ()
serveBatch listener remaining = do
    withAcceptedTcpConnection listener reply
    serveBatch listener (remaining - 1)

httpAsHostError : HttpError -> HostError
httpAsHostError (HttpHostError hostError) = hostError
httpAsHostError error = HostError InvalidArgument "HTTP" (show error)

fetchOne : String -> Task String
fetchOne url = do
    response <- mapError httpAsHostError (get url)
    case std.http.responseText response of
        Right text => pureIO text
        | Left error => failIO (httpAsHostError error)

runClients : Port -> Task [String]
runClients port =
    let url =
        "http://127.0.0.1:" <> show port <> "/message"
    in mapError concurrentHostError (mapConcurrentlyBoundedOrdered 3 fetchOne [url, url, url])

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
