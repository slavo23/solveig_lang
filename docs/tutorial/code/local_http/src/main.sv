import std.bytes(*)
import std.channel(*)
import std.http(*)
import std.list(length)
import std.net(*)
import std.task(*)

responseBody : String
responseBody = "{\"message\":\"hello from Solveig\"}"

responseText : String
responseText =
    "HTTP/1.1 200 OK\u{d}\n"
        <> "Content-Type: application/json\u{d}\n"
        <> "Content-Length: " <> show (bytesLength (stringToBytes responseBody)) <> "\u{d}\n"
        <> "Connection: close\u{d}\n\u{d}\n"
        <> responseBody

netToHost : NetError -> HostError
netToHost (NetHostError _ hostError) = hostError

recvBytesTask : Socket -> Int -> Task Bytes
recvBytesTask socket maximum = mapError netToHost (std.net.recvBytes socket maximum)

sendBytesTask : Socket -> Bytes -> Task ()
sendBytesTask socket bytes = mapError netToHost (std.net.sendBytes socket bytes)

closeSocketTask : Socket -> Task ()
closeSocketTask socket = mapError netToHost (std.net.closeSocket socket)

acceptTask : Socket -> Task Socket
acceptTask listener = mapError netToHost (std.net.accept listener)

listenTask : Int -> Task (Socket, Int)
listenTask port = mapError netToHost (std.net.listen port)

reply : Socket -> Task ()
reply connection = do
    _ <- recvBytesTask connection 4096
    sendBytesTask connection (stringToBytes responseText)

finishConnection : Chan () -> Socket -> Task ()
finishConnection finished connection = do
    closeSocketTask connection
    std.channel.send finished ()

serveConnection : Chan () -> Socket -> Task ()
serveConnection finished connection =
    finally (reply connection) (finishConnection finished connection)

waitFor : Chan () -> Int -> Task ()
waitFor _ 0 = pureIO ()
waitFor finished remaining = do
    _ <- recv finished
    waitFor finished (remaining - 1)

acceptBatch : Socket -> Chan () -> Int -> Int -> Task ()
acceptBatch _ finished 0 total = waitFor finished total
acceptBatch listener finished remaining total = do
    connection <- acceptTask listener
    withThread
        (serveConnection finished connection)
        (fun _ => acceptBatch listener finished (remaining - 1) total)

serveBatch : Socket -> Int -> Task ()
serveBatch listener count = do
    finished <- newChan
    acceptBatch listener finished count count
    closeChan finished

httpAsHostError : HttpError -> HostError
httpAsHostError (HttpHostError hostError) = hostError
httpAsHostError error = HostError InvalidArgument "HTTP" (show error)

fetchOne : String -> Task String
fetchOne url = do
    response <- mapError httpAsHostError (fetch url)
    case std.http.responseText response of
        Right text => pureIO text
        | Left error => failIO (httpAsHostError error)

runClients : Int -> Task [String]
runClients port =
    let url = "http://127.0.0.1:" <> show port <> "/message" in
    mapError concurrentHostError (mapConcurrentlyBounded 3 fetchOne [url, url, url])

main : Task ()
main =
    bracket (listenTask 0) (fun (listener, _) => closeSocketTask listener) (fun listenerInfo =>
        case listenerInfo of
            (listener, port) =>
                withThread
                    (serveBatch listener 3)
                    (fun _ => do
                        bodies <- runClients port
                        printLine ("responses: " <> show (length bodies))
                        printLine (case bodies of first :: _ => first | [] => "none")))
