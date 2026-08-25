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

reply : Socket -> Task ()
reply connection = do
    _ <- recvBytes connection 4096
    sendBytes connection (stringToBytes responseText)

finishConnection : Chan () -> Socket -> Task ()
finishConnection finished connection = do
    closeSocket connection
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
    connection <- accept listener
    withThread
        (serveConnection finished connection)
        (fun _ => acceptBatch listener finished (remaining - 1) total)

serveBatch : Socket -> Int -> Task ()
serveBatch listener count = do
    finished <- newChan
    acceptBatch listener finished count count
    closeChan finished

httpAsIOError : HttpError -> IOError
httpAsIOError (HttpIoError ioError) = ioError
httpAsIOError (InvalidUrl message) = HttpTransportError message
httpAsIOError (InvalidHeaderName name) = HttpTransportError name
httpAsIOError (InvalidHeaderValue value) = HttpTransportError value
httpAsIOError (InvalidHttpLimit name value) = HttpTransportError name

fetchOne : String -> Task String
fetchOne url = do
    response <- mapError httpAsIOError (fetch url)
    case std.http.responseText response of
        Right text => pureIO text
        | Left error => failIO (httpAsIOError error)

runClients : Int -> Task [String]
runClients port =
    let url = "http://127.0.0.1:" <> show port <> "/message" in
    mapConcurrentlyBounded 3 fetchOne [url, url, url]

main : Task ()
main =
    withListener 0 (fun listenerInfo =>
        case listenerInfo of
            (listener, port) =>
                withThread
                    (serveBatch listener 3)
                    (fun _ => do
                        bodies <- runClients port
                        printLine ("responses: " <> show (length bodies))
                        printLine (case bodies of first :: _ => first | [] => "none")))
