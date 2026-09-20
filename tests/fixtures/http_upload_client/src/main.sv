import std.bytes(*)
import std.channel(newChan, recv, send)
import std.env(getArgs)
import std.http(*)
import std.json(Json(..))
import std.map(singleton)
import std.path(Path, pathFromString)
import std.stream as Streams
import std.task(withThread)
import std.time(seconds, sleep)

body : Streams.Stream () HttpError Bytes
body =
    Streams.fromList
        [stringToBytes "ab", bytesSlice (stringToBytes "ignored") 0 0, stringToBytes "cde"]

failingBodyStep : Int -> IO HttpError (Maybe (Bytes, Int))
failingBodyStep step =
    if step == 0
        then pureIO $ Just (stringToBytes "partial", 1)
        else failIO $ HttpIOError $ IOError OtherError "fixtureUpload" "planned"

failingBody : Streams.Stream () HttpError Bytes
failingBody = Streams.unfold failingBodyStep 0

slowBodyStep : Chan () -> Int -> IO HttpError (Maybe (Bytes, Int))
slowBodyStep entered _ = do
    mapError HttpIOError $ send entered ()
    mapError HttpIOError $ sleep $ seconds 3600
    pureIO Nothing

slowBody : Chan () -> Streams.Stream () HttpError Bytes
slowBody entered = Streams.unfold (slowBodyStep entered) 0

appendChunk : BytesBuilder -> Bytes -> BytesBuilder
appendChunk builder bytes = builder <> bytesBuilder bytes

collect : forall scope. Streams.Stream scope HttpError Bytes -> IO HttpError Bytes
collect stream = fmap buildBytes (Streams.fold appendChunk emptyBytesBuilder stream)

say : String -> IO HttpError ()
say = mapError HttpIOError . printLine

httpAsIOError : HttpError -> IOError
httpAsIOError problem =
    case problem of
        HttpIOError host => host
        | _ => IOError InvalidArgument "HTTP" (show problem)

sayResponse : String -> Response -> IO HttpError ()
sayResponse label response =
    case responseText response of
        Left problem => say (label <> ":" <> show problem)
        | Right text =>
            say (label <> ":" <> show (statusCode (responseStatus response)) <> ":" <> text)

expectReplay : String -> IO HttpError Response -> IO HttpError ()
expectReplay label action =
    catchError
        (do
            _ <- action
            say (label <> ":unexpected")
        ) (fun problem =>
            case problem of
                BodyReplayError _ => say (label <> ":blocked")
                | _ => failIO problem
        )

expectManagedHeader : String -> IO HttpError Response -> IO HttpError ()
expectManagedHeader label action =
    catchError
        (do
            _ <- action
            say (label <> ":unexpected")
        ) (fun problem =>
            case problem of
                ManagedRequestHeader name => say (label <> ":" <> name)
                | _ => failIO problem
        )

expectSourceFailure : IO HttpError Response -> IO HttpError ()
expectSourceFailure action =
    catchError
        (do
            _ <- action
            say "source-failure:unexpected"
        ) (fun problem =>
            case problem of
                HttpIOError host =>
                    if ioErrorOperation host == "fixtureUpload"
                        then say "source-failure:planned"
                        else failIO problem
                | _ => failIO problem
        )

cancelUpload : String -> IO HttpError ()
cancelUpload url = do
    entered <- mapError HttpIOError newChan
    mapError HttpIOError $
        withThread
            (mapError httpAsIOError $ requestStream Post (slowBody entered) url)
            (fun _ => recv entered)
    say "cancel-upload:closed"

run : String -> Path -> IO HttpError ()
run base payload = do
    queryResponse <-
        getWith
            (withQuery [("q", "snow ☃"), ("tag", "a b"), ("tag", "x/y")] defaultOptions)
            (base <> "/query?fixed=yes")
    sayResponse "query" queryResponse
    textResponse <- postWith (withTextBody "hello ✓" defaultOptions) (base <> "/text")
    sayResponse "text" textResponse
    jsonResponse <-
        postWith
            (withHeader "Content-Type" "application/vnd.solveig+json" $
                withJsonBody (JsonObject $ singleton "name" $ JsonString "Ada") defaultOptions)
            (base <> "/json")
    sayResponse "json" jsonResponse
    formResponse <-
        postWith
            (withFormBody [("name", "Ada Lovelace"), ("tag", "a/b"), ("tag", "✓")] defaultOptions)
            (base <> "/form")
    sayResponse "form" formResponse
    builderResponse <-
        putWith
            (withBuilderBody
                (bytesBuilder (stringToBytes "build") <> bytesBuilder (stringToBytes "er"))
                defaultOptions)
            (base <> "/builder")
    sayResponse "builder" builderResponse
    fileResponse <- putWith (withFileBody payload defaultOptions) (base <> "/file")
    sayResponse "file" fileResponse
    redirectResponse <-
        postWith (withTextBody "repeat me" defaultOptions) (base <> "/redirect-repeat")
    sayResponse "redirect" redirectResponse
    expectReplay
        "no-replay"
        (postWith
            (withReplayPolicy NoReplay $ withTextBody "do not repeat" defaultOptions)
            (base <> "/redirect-no-replay"))
    crossOriginResponse <-
        getWith
            (withHeader "Authorization" "Bearer secret" $
                withHeader "Cookie" "session=secret" $
                withHeader "Proxy-Authorization" "proxy secret" $
                withHeader "X-Trace" "kept" defaultOptions)
            (base <> "/cross-origin")
    sayResponse "cross-origin" crossOriginResponse
    expectManagedHeader
        "managed"
        (postWith (withHeader "Content-Length" "1" defaultOptions) (base <> "/never-sent"))
    strict <- requestStream Post body (base <> "/strict")
    sayResponse "strict" strict
    streamed <-
        withStreamResponseWith
            (withChunkBytes 1 defaultOptions)
            Post
            body
            (base <> "/scoped")
            (fun status _ responseBody => fmap (fun bytes => (status, bytes)) (collect responseBody)
            )
    case streamed of
        (status, bytes) =>
            case bytesToString bytes of
                Left problem => say ("scoped:" <> show problem)
                | Right text => say ("scoped:" <> show (statusCode status) <> ":" <> text)
    expectSourceFailure $ requestStream Post failingBody (base <> "/source-failure")
    cancelUpload $ base <> "/cancel-upload"
    expectReplay "stream-replay" $ requestStream Post body (base <> "/stream-redirect")

main : Task ()
main = do
    args <- getArgs
    case args of
        base :: payload :: _ =>
            case pathFromString payload of
                Left problem => printLine $ show problem
                | Right path => mapError httpAsIOError $ catchError (run base path) (say . show)
        | [] => printLine "missing fixture URL"
        | _ => printLine "missing fixture payload path"
