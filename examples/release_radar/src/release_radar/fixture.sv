import std.bytes(*)
import std.list(*)
import std.net(*)
import std.task(*)
import std.time(*)

emptyBytes : Bytes
emptyBytes = std.bytes.stringToBytes ""

netToHost : std.net.NetError -> HostError
netToHost (std.net.NetHostError _ hostError) = hostError
netToHost error = HostError InvalidArgument "network" (show error)

hostToNet : HostError -> std.net.NetError
hostToNet hostError = std.net.NetHostError "fixture" hostError

fixtureBody : String
fixtureBody =
    "{\"tag_name\":\"v-local\",\"published_at\":\"2026-08-20\",\"html_url\":\"https://example.test/release\"}"

fixtureResponse : String
fixtureResponse =
    "HTTP/1.1 200 OK\u{d}\n"
        <> "Content-Type: application/json\u{d}\n"
        <> "ETag: local-etag\u{d}\n"
        <> "Content-Length: "
        <> show (std.bytes.bytesLength (std.bytes.stringToBytes fixtureBody))
        <> "\u{d}\nConnection: close\u{d}\n\u{d}\n"
        <> fixtureBody

findHeaderEnd : Bytes -> Int -> Maybe Int
findHeaderEnd bytes index =
    if index + 4 > std.bytes.bytesLength bytes
        then Nothing
        else if and
                (byteAtIs bytes index 13)
                (and
                    (byteAtIs bytes (index + 1) 10)
                    (and (byteAtIs bytes (index + 2) 13) (byteAtIs bytes (index + 3) 10)))
                then Just (index + 4)
                else findHeaderEnd bytes (index + 1)

byteAtIs : Bytes -> Int -> Int -> Bool
byteAtIs bytes index expected =
    case std.bytes.bytesIndex bytes index of
        Just value => value == expected
        | Nothing => False

readRequestHeaders : forall scope. TcpConnection scope -> Bytes -> IO std.net.NetError Bytes
readRequestHeaders connection accumulated =
    case findHeaderEnd accumulated 0 of
        Just _ => pureIO accumulated
        | Nothing => do
            chunk <- std.net.receiveTcpBytes connection 1024
            if std.bytes.bytesLength chunk == 0
                then pureIO accumulated
                else readRequestHeaders connection (std.bytes.bytesAppend accumulated chunk)

sendFixtureResponse : forall scope. TcpConnection scope -> Bytes -> IO std.net.NetError ()
sendFixtureResponse connection request = do
    mapError hostToNet (std.time.sleep (std.time.milliseconds (requestDelay request)))
    std.net.sendTcpBytes connection (std.bytes.stringToBytes fixtureResponse)

requestDelay : Bytes -> Int
requestDelay request = 30 + mod (byteSum request 0 0) 170

byteSum : Bytes -> Int -> Int -> Int
byteSum bytes index total =
    case std.bytes.bytesIndex bytes index of
        Nothing => total
        | Just value => byteSum bytes (index + 1) (total + value)

serveConnection : forall scope. TcpConnection scope -> IO std.net.NetError ()
serveConnection connection = do
    request <- readRequestHeaders connection emptyBytes
    sendFixtureResponse connection request

serveOne : forall scope. TcpListener scope -> Int -> IO std.net.NetError ()
serveOne listener _ = std.net.withAcceptedTcpConnection listener serveConnection

concurrentToNet : std.task.ConcurrentError std.net.NetError -> std.net.NetError
concurrentToNet (std.task.ConcurrentInfrastructure hostError) =
    std.net.NetHostError "fixture server" hostError
concurrentToNet (std.task.ConcurrentWorker networkError) = networkError

serveFixture : forall scope. TcpListener scope -> Int -> IO std.net.NetError ()
serveFixture listener total =
    fmap
        (fun _ => ())
        (mapError
            concurrentToNet
            (std.task.mapConcurrentlyBoundedOrdered 8 (serveOne listener) (std.list.range 0 total)))

withFixture : forall a. Int -> (String -> Task a) -> Task a
withFixture requestCount action =
    mapError netToHost (std.net.withTcpListener std.net.anyPort (runFixture requestCount action))

runFixture :
    forall a scope. Int -> (String -> Task a) -> TcpListener scope -> Port -> IO std.net.NetError a
runFixture requestCount action listener port =
    std.task.withThread
        (serveFixture listener requestCount)
        (fun _ => mapError hostToNet (action ("http://127.0.0.1:" <> show port)))
