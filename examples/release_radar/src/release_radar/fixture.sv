import std.bytes(*)
import std.list(*)
import std.net(*)
import std.task(*)
import std.time(*)

emptyBytes : Bytes
emptyBytes = std.bytes.stringToBytes ""

fixtureBody : String
fixtureBody =
    "{\"tag_name\":\"v-local\",\"published_at\":\"2026-08-20\",\"html_url\":\"https://example.test/release\"}"

fixtureResponse : String
fixtureResponse =
    "HTTP/1.1 200 OK\u{d}\n" <>
        "Content-Type: application/json\u{d}\n" <>
        "ETag: local-etag\u{d}\n" <>
        "Content-Length: " <>
        show (std.bytes.bytesLength (std.bytes.stringToBytes fixtureBody)) <>
        "\u{d}\nConnection: close\u{d}\n\u{d}\n" <>
        fixtureBody

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

readRequestHeaders : Socket -> Bytes -> Task Bytes
readRequestHeaders connection accumulated =
    case findHeaderEnd accumulated 0 of
        Just _ => pureIO accumulated
        | Nothing =>
            do
                chunk <- std.net.recvBytes connection 1024
                if std.bytes.bytesLength chunk == 0
                    then pureIO accumulated
                    else readRequestHeaders connection (std.bytes.bytesAppend accumulated chunk)

sendFixtureResponse : Socket -> Bytes -> Task ()
sendFixtureResponse connection request = do
    std.time.sleep (requestDelay request)
    std.net.sendBytes connection (std.bytes.stringToBytes fixtureResponse)

requestDelay : Bytes -> Int
requestDelay request = 30 + mod (byteSum request 0 0) 170

byteSum : Bytes -> Int -> Int -> Int
byteSum bytes index total =
    case std.bytes.bytesIndex bytes index of
        Nothing => total
        | Just value => byteSum bytes (index + 1) (total + value)

serveOne : Socket -> Int -> Task ()
serveOne listener _ =
    std.task.bracket
        (std.net.accept listener)
        std.net.closeSocket
        (fun connection =>
            do
                request <- readRequestHeaders connection emptyBytes
                sendFixtureResponse connection request
        )

serveFixture : Socket -> Int -> Task ()
serveFixture listener total =
    fmap
        (fun _ => ())
        (std.task.mapConcurrentlyBounded 8 (serveOne listener) (std.list.range 0 total))

withFixture : forall a. Int -> (String -> Task a) -> Task a
withFixture requestCount action = do
    listenerInfo <- std.net.listen 0
    case listenerInfo of
        (listener, port) =>
            std.task.finally
                (std.task.withThread
                    (serveFixture listener requestCount)
                    (fun server => action ("http://127.0.0.1:" <> show port)))
                (std.net.closeSocket listener)
