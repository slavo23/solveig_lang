import std.bytes
import std.http
import std.io
import std.json
import std.map
import std.net
import std.parser
import std.task
import std.testing
import std.text
import std.time
import std.vector

fixtureBody : String
fixtureBody = "{\"names\":[\"Ada\",\"Grace\",\"Linus\",\"Margaret\"]}"

fixtureResponse : String
fixtureResponse =
    "HTTP/1.1 200 OK\u{d}\nContent-Type: application/json\u{d}\nContent-Length: "
        <> show (std.bytes.bytesLength (std.bytes.stringToBytes fixtureBody))
        <> "\u{d}\nConnection: close\u{d}\n\u{d}\n"
        <> fixtureBody

isDigit : Char -> Bool
isDigit character = and (character >= '0') (character <= '9')

digitValue : Char -> Int
digitValue character = std.text.charToCodePoint character - 48

digitsValue : [Char] -> Int
digitsValue = foldl (fun total character => total * 10 + digitValue character) 0

workersParser : std.parser.Parser std.parser.TextInput () Int
workersParser =
    std.parser.keepRight
        (std.parser.tokens
            std.parser.textInputOps
            "workers="
            (std.text.stringToChars "workers="))
        (std.parser.mapResult
            digitsValue
            (std.parser.some
                std.parser.textInputOps
                (std.parser.satisfy std.parser.textInputOps "digit" isDigit)))

parseWorkers : String -> Result std.testing.TestFailure Int
parseWorkers text =
    std.parser.foldReply
        (fun _ value _ => Ok value)
        (fun _ _ _ => Err (TestFailure "settings parser rejected valid input"))
        (fun _ _ _ => Err (TestFailure "settings parser requested more input"))
        (std.parser.runComplete
            std.parser.textInputOps
            workersParser
            (std.parser.textInput (std.text.trimAscii text)))

httpAsIOError : std.http.HttpError -> IOError
httpAsIOError (HttpIoError ioError) = ioError
httpAsIOError (InvalidUrl _) = HttpTransportError "invalid URL"
httpAsIOError (InvalidHeaderName _) = HttpTransportError "invalid header name"
httpAsIOError (InvalidHeaderValue _) = HttpTransportError "invalid header value"
httpAsIOError (InvalidHttpLimit _ _) = HttpTransportError "invalid HTTP limit"

serveOnce : Socket -> Task ()
serveOnce listener =
    std.net.withAcceptedConnection listener (fun connection => do
        _ <- std.net.recvBytes connection 4096
        std.net.sendBytes connection (std.bytes.stringToBytes fixtureResponse))

withFixture : forall a. (String -> Task a) -> Task a
withFixture action =
    std.net.withListener 0 (fun listenerInfo =>
        case listenerInfo of
            MkPair listener port =>
                std.task.withThread
                    (serveOnce listener)
                    (fun _ => action ("http://127.0.0.1:" <> show port)))

fetchNames : String -> Task (Result std.testing.TestFailure (Pair Int (std.vector.Vector String)))
fetchNames base = do
    response <- mapError httpAsIOError (std.http.fetch (base <> "/names"))
    let status = std.http.statusCode (std.http.responseStatus response)
    pureIO
        (std.parser.foldReply
            (fun _ value _ => namesFromJson status value)
            (fun _ _ _ => Err (TestFailure "HTTP body is invalid JSON"))
            (fun _ _ _ => Err (TestFailure "HTTP JSON requested more input"))
            (std.json.parseJson std.json.defaultJsonLimits (std.http.responseBody response)))

namesFromJson : Int -> std.json.Json -> Result std.testing.TestFailure (Pair Int (std.vector.Vector String))
namesFromJson status (JsonObject object) =
    case std.map.lookup "names" object of
        Just (JsonArray values) =>
            case jsonStrings (std.vector.toList values) of
                Just names => Ok (MkPair status (std.vector.fromList names))
                | Nothing => Err (TestFailure "names array contains a non-string")
        | _ => Err (TestFailure "JSON object has no names array")
namesFromJson _ _ = Err (TestFailure "HTTP JSON is not an object")

jsonStrings : [std.json.Json] -> Maybe [String]
jsonStrings [] = Just []
jsonStrings (JsonString value :: rest) =
    case jsonStrings rest of
        Nothing => Nothing
        | Just values => Just (value :: values)
jsonStrings (_ :: _) = Nothing

invalidUrlIsTyped : Task Bool
invalidUrlIsTyped =
    catchError
        (fmap (fun _ => False) (std.http.fetch "not-a-url"))
        (fun httpError => pureIO
            (case httpError of InvalidUrl _ => True | _ => False))

resultOrExit : Result std.testing.TestFailure a -> Task a
resultOrExit (Ok value) = pureIO value
resultOrExit (Err failure) = error (std.testing.failureMessage failure)

isNothing : Maybe a -> Bool
isNothing Nothing = True
isNothing (Just _) = False

main : Task ()
main = do
    settings <- std.io.readFileLimited "fixture/settings.txt" 1024
    workers <- resultOrExit (parseWorkers settings)
    fetched <- withFixture fetchNames
    response <- resultOrExit fetched
    typedInvalid <- invalidUrlIsTyped
    cancelled <- std.task.timeout 5 (std.time.sleep 100)
    case response of
        MkPair status names =>
            resultOrExit
                (std.testing.checkAll
                    [ std.testing.assertEqual "worker count" 4 workers
                    , std.testing.assertEqual "HTTP status" 200 status
                    , std.testing.assertEqual "JSON vector length" 4 (std.vector.length names)
                    , std.testing.assert "invalid URL is typed" typedInvalid
                    , std.testing.assert "timeout cancels" (isNothing cancelled)
                    ])
    printLine "stdlib practical: pass"
