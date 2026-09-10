import std.bytes(*)
import std.fs(FsError(..))
import std.http(*)
import std.json(*)
import std.map(*)
import std.net(*)
import std.parser(*)
import std.path(Path, pathFromString)
import std.task(*)
import std.testing(*)
import std.text(*)
import std.time(*)
import std.vector(*)

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
        (std.parser.tokens std.parser.textInputOps "workers=" (std.text.stringToChars "workers="))
        (std.parser.mapResult
            digitsValue
            (std.parser.some
                std.parser.textInputOps
                (std.parser.satisfy std.parser.textInputOps "digit" isDigit)))

parseWorkers : String -> Either std.testing.TestFailure Int
parseWorkers text =
    std.parser.foldReply
        (fun _ value _ => Right value)
        (fun _ _ _ => Left (TestFailure "settings parser rejected valid input"))
        (fun _ _ _ => Left (TestFailure "settings parser requested more input"))
        (std.parser.runComplete
            std.parser.textInputOps
            workersParser
            (std.parser.textInput (std.text.trimAscii text)))

httpAsHostError : std.http.HttpError -> HostError
httpAsHostError (HttpHostError hostError) = hostError
httpAsHostError error = HostError InvalidArgument "HTTP" (show error)

netToHost : std.net.NetError -> HostError
netToHost (std.net.NetHostError _ hostError) = hostError
netToHost error = HostError InvalidArgument "network" (show error)

hostToNet : HostError -> std.net.NetError
hostToNet hostError = std.net.NetHostError "fixture" hostError

serveConnection : forall scope. TcpConnection scope -> IO std.net.NetError ()
serveConnection connection = do
    _ <- std.net.receiveTcpBytes connection 4096
    std.net.sendTcpBytes connection (std.bytes.stringToBytes fixtureResponse)

serveOnce : forall scope. TcpListener scope -> IO std.net.NetError ()
serveOnce listener = std.net.withAcceptedTcpConnection listener serveConnection

withFixture : forall a. (String -> Task a) -> Task a
withFixture action = mapError netToHost (std.net.withTcpListener anyPort (runFixture action))

runFixture :
    forall a scope. (String -> Task a) -> TcpListener scope -> Port -> IO std.net.NetError a
runFixture action listener port =
    std.task.withThread
        (serveOnce listener)
        (fun _ => mapError hostToNet (action ("http://127.0.0.1:" <> show port)))

fetchNames : String -> Task (Either std.testing.TestFailure (Int, (std.vector.Vector String)))
fetchNames base = do
    response <- mapError httpAsHostError (std.http.get (base <> "/names"))
    let status = std.http.statusCode (std.http.responseStatus response)
    pureIO
        (std.parser.foldReply
            (fun _ value _ => namesFromJson status value)
            (fun _ _ _ => Left (TestFailure "HTTP body is invalid JSON"))
            (fun _ _ _ => Left (TestFailure "HTTP JSON requested more input"))
            (std.json.parseJson std.json.defaultJsonLimits (std.http.responseBody response)))

namesFromJson :
    Int -> std.json.Json -> Either std.testing.TestFailure (Int, (std.vector.Vector String))
namesFromJson status (JsonObject object) =
    case std.map.lookup "names" object of
        Just (JsonArray values) =>
            case jsonStrings (std.vector.toList values) of
                Just names => Right (status, (std.vector.fromList names))
                | Nothing => Left (TestFailure "names array contains a non-string")
        | _ => Left (TestFailure "JSON object has no names array")
namesFromJson _ _ = Left (TestFailure "HTTP JSON is not an object")

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
        (fmap (fun _ => False) (std.http.get "not-a-url"))
        (fun httpError =>
            pureIO
                (case httpError of
                    InvalidUrl _ => True
                    | _ => False
                )
        )

resultOrExit : Either std.testing.TestFailure a -> Task a
resultOrExit (Right value) = pureIO value
resultOrExit (Left failure) = error (show failure)

isNothing : Maybe a -> Bool
isNothing Nothing = True
isNothing (Just _) = False

fsToHost : FsError -> HostError
fsToHost (FsHostError _ hostError) = hostError
fsToHost error = HostError InvalidArgument "filesystem" (show error)

requiredPath : String -> Path
requiredPath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error (show pathError)

main : Task ()
main = do
    settings <-
        mapError fsToHost (std.fs.readFileLimited (requiredPath "fixture/settings.txt") 1024)
    workers <- resultOrExit (parseWorkers settings)
    fetched <- withFixture fetchNames
    response <- resultOrExit fetched
    typedInvalid <- invalidUrlIsTyped
    cancelled <-
        std.task.timeout (std.time.milliseconds 5) (std.time.sleep (std.time.milliseconds 100))
    case response of
        (status, names) =>
            resultOrExit
                (std.testing.checkAll
                    [
                        std.testing.assertEqual "worker count" 4 workers,
                        std.testing.assertEqual "HTTP status" 200 status,
                        std.testing.assertEqual "JSON vector length" 4 (std.vector.length names),
                        std.testing.assert "invalid URL is typed" typedInvalid,
                        std.testing.assert "timeout cancels" (isNothing cancelled)
                    ])
    printLine "stdlib practical: pass"
