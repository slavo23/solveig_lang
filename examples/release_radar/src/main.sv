import std.bytes
import std.channel
import std.env(getArgs)
import std.fs
import std.http
import std.io
import std.json
import std.list
import std.map
import std.net
import std.parser
import std.task
import std.text
import std.thread
import std.time

data CacheEntry = CacheEntry String String

data ReleaseResult =
    ReleaseFound String String String String String
  | ReleaseUnchanged String String
  | ReleaseFailed String String

cachePath : String
cachePath = ".release-radar-cache"

httpLimits : std.http.HttpLimits
httpLimits = std.http.HttpLimits 65536 1048576 10000 (std.http.FollowRedirects 3)

requiredHeader : String -> String -> std.http.Header
requiredHeader name value =
    case std.http.header name value of
        Ok result => result
        | Err _ => error "release-radar constant header is invalid"

requiredUrl : String -> std.http.Url
requiredUrl spelling =
    case std.http.urlFromString spelling of
        Ok result => result
        | Err _ => error ("release-radar constructed invalid URL: " <> spelling)

splitLines : String -> [String]
splitLines text = splitLinesChars (std.text.stringToChars text) [] []

splitLinesChars : [Char] -> [Char] -> [String] -> [String]
splitLinesChars [] reversedLine reversedLines =
    std.list.reverse
        (if charsEmpty reversedLine
            then reversedLines
            else std.text.charsToString (std.list.reverse reversedLine) :: reversedLines)
splitLinesChars (character :: rest) reversedLine reversedLines =
    if character == '\n' then
        splitLinesChars
            rest
            []
            (if charsEmpty reversedLine
                then reversedLines
                else std.text.charsToString (std.list.reverse reversedLine) :: reversedLines)
    else
        splitLinesChars rest (character :: reversedLine) reversedLines

charsEmpty : [Char] -> Bool
charsEmpty [] = True
charsEmpty _ = False

listLength : forall a. [a] -> Int
listLength [] = 0
listLength (_ :: rest) = 1 + listLength rest

splitTab : String -> Maybe (Pair String String)
splitTab text = splitTabChars (std.text.stringToChars text) []

splitTabChars : [Char] -> [Char] -> Maybe (Pair String String)
splitTabChars [] _ = Nothing
splitTabChars (character :: rest) reversedLeft =
    if std.text.charToCodePoint character == 9 then
        Just
            (MkPair
                (std.text.charsToString (std.list.reverse reversedLeft))
                (std.text.charsToString rest))
    else splitTabChars rest (character :: reversedLeft)

parseCache : String -> [CacheEntry]
parseCache text = parseCacheLines (splitLines text)

parseCacheLines : [String] -> [CacheEntry]
parseCacheLines [] = []
parseCacheLines (line :: rest) =
    case splitTab line of
        Nothing => parseCacheLines rest
        | Just (MkPair repository etag) => CacheEntry repository etag :: parseCacheLines rest

cacheEtag : String -> [CacheEntry] -> Maybe String
cacheEtag _ [] = Nothing
cacheEtag repository (CacheEntry candidate etag :: rest) =
    if repository == candidate then Just etag else cacheEtag repository rest

loadCache : Task [CacheEntry]
loadCache = do
    exists <- std.fs.doesFileExist cachePath
    if exists then fmap parseCache (std.io.readFileLimited cachePath 65536)
    else pureIO []

repositoryRequest : String -> [CacheEntry] -> String -> std.http.Request
repositoryRequest base cache repository =
    let initial = std.http.get
        (requiredUrl (base <> "/repos/" <> repository <> "/releases/latest")) in
    let identified = std.http.withHeader
        (requiredHeader "User-Agent" "solveig-release-radar/0.1") initial in
    let accepting = std.http.withHeader
        (requiredHeader "Accept" "application/vnd.github+json") identified in
    case cacheEtag repository cache of
        Nothing => accepting
        | Just etag => std.http.withHeader (requiredHeader "If-None-Match" etag) accepting

fetchRelease : String -> [CacheEntry] -> String -> Task ReleaseResult
fetchRelease base cache repository =
    catchError
        (fmap
            (decodeRelease repository (cacheEtag repository cache))
            (std.http.request httpLimits (repositoryRequest base cache repository)))
        (fun requestError => pureIO (ReleaseFailed repository (describeHttpError requestError)))

decodeRelease : String -> Maybe String -> std.http.Response -> ReleaseResult
decodeRelease repository cached response =
    let code = std.http.statusCode (std.http.responseStatus response) in
    if code == 304 then
        case cached of
            Just etag => ReleaseUnchanged repository etag
            | Nothing => ReleaseFailed repository "server returned 304 without a cached ETag"
    else if code /= 200 then
        ReleaseFailed repository ("HTTP status " <> show code)
    else
        let etag = case std.http.findHeader "etag" response of
            Just value => value
            | Nothing => ""
        in decodeReleaseJson repository etag (std.http.responseBody response)

decodeReleaseJson : String -> String -> Bytes -> ReleaseResult
decodeReleaseJson repository etag body =
    std.parser.foldReply
        (fun consumption value rest => releaseFromJson repository etag value)
        (fun consumption rest parseError => ReleaseFailed repository "invalid release JSON")
        (fun consumption rest continue => ReleaseFailed repository "incomplete release JSON")
        (std.json.parseJson std.json.defaultJsonLimits body)

releaseFromJson : String -> String -> std.json.Json -> ReleaseResult
releaseFromJson repository etag (JsonObject object) =
    case stringField "tag_name" object of
        Nothing => ReleaseFailed repository "JSON has no tag_name"
        | Just tag =>
            case stringField "published_at" object of
                Nothing => ReleaseFailed repository "JSON has no published_at"
                | Just published =>
                    case stringField "html_url" object of
                        Nothing => ReleaseFailed repository "JSON has no html_url"
                        | Just url => ReleaseFound repository tag published url etag
releaseFromJson repository _ _ = ReleaseFailed repository "release JSON is not an object"

stringField : String -> std.map.Map String std.json.Json -> Maybe String
stringField name object =
    case std.map.lookup name object of
        Just (JsonString value) => Just value
        | _ => Nothing

describeHttpError : std.http.HttpError -> String
describeHttpError (InvalidUrl _) = "invalid URL"
describeHttpError (InvalidHeaderName _) = "invalid header name"
describeHttpError (InvalidHeaderValue _) = "invalid header value"
describeHttpError (InvalidHttpLimit name value) = "invalid " <> name <> ": " <> show value
describeHttpError (HttpIoError ioError) =
    case ioError of
        HttpTransportError message => "transport: " <> message
        | HttpTlsError message => "TLS: " <> message
        | HttpTimeoutError _ => "request timed out"
        | HttpRedirectError message => "redirect: " <> message
        | HttpHeadersTooLarge maximum => "response headers exceed " <> show maximum <> " bytes"
        | HttpBodyTooLarge maximum => "response body exceeds " <> show maximum <> " bytes"
        | _ => "unexpected IO error"

worker : String -> [CacheEntry] -> Chan (Pair Int String) -> Chan (Pair Int ReleaseResult) -> Chan () -> Task ()
worker base cache jobs results finished =
    std.task.finally
        (catchError
            (workerLoop base cache jobs results)
            (fun channelClosed => pureIO ()))
        (std.channel.send finished ())

workerLoop : String -> [CacheEntry] -> Chan (Pair Int String) -> Chan (Pair Int ReleaseResult) -> Task ()
workerLoop base cache jobs results = do
    job <- std.channel.recv jobs
    case job of
        MkPair index repository => do
            result <- fetchRelease base cache repository
            std.channel.send results (MkPair index result)
            workerLoop base cache jobs results

startWorkers : Int -> String -> [CacheEntry] -> Chan (Pair Int String) -> Chan (Pair Int ReleaseResult) -> Chan () -> Task ()
startWorkers count base cache jobs results finished =
    if count == 0 then pureIO ()
    else do
        thread <- std.thread.spawn (worker base cache jobs results finished)
        startWorkers (count - 1) base cache jobs results finished

enqueueRepositories : Chan (Pair Int String) -> [String] -> Int -> Task ()
enqueueRepositories jobs [] _ = std.channel.closeChan jobs
enqueueRepositories jobs (repository :: rest) index = do
    std.channel.send jobs (MkPair index repository)
    enqueueRepositories jobs rest (index + 1)

collectResults : Chan (Pair Int ReleaseResult) -> Int -> std.map.Map Int ReleaseResult -> Task (std.map.Map Int ReleaseResult)
collectResults results remaining ordered =
    if remaining == 0 then pureIO ordered
    else do
        indexed <- std.channel.recv results
        case indexed of
            MkPair index result =>
                collectResults results (remaining - 1) (std.map.insert index result ordered)

awaitWorkers : Chan () -> Int -> Task ()
awaitWorkers finished remaining =
    if remaining == 0 then pureIO ()
    else do
        completion <- std.channel.recv finished
        awaitWorkers finished (remaining - 1)

runConcurrent : String -> [CacheEntry] -> [String] -> Task [ReleaseResult]
runConcurrent base cache repositories =
    let total = listLength repositories in
    if total == 0 then pureIO []
    else do
        jobs <- std.channel.newBoundedChan total
        results <- std.channel.newBoundedChan total
        finished <- std.channel.newBoundedChan 4
        startWorkers 4 base cache jobs results finished
        enqueueRepositories jobs repositories 0
        ordered <- collectResults results total std.map.empty
        awaitWorkers finished 4
        std.channel.closeChan results
        std.channel.closeChan finished
        pureIO (std.map.values ordered)

renderResult : ReleaseResult -> String
renderResult (ReleaseFound repository tag published url _) =
    repository <> "  " <> tag <> "  " <> published <> "\n  " <> url
renderResult (ReleaseUnchanged repository _) = repository <> "  unchanged (HTTP 304)"
renderResult (ReleaseFailed repository message) = repository <> "  ERROR: " <> message

printResults : [ReleaseResult] -> Task ()
printResults [] = pureIO ()
printResults (result :: rest) = do
    printLine (renderResult result)
    printResults rest

cacheText : [ReleaseResult] -> String
cacheText results = cacheTextInto results ""

cacheTextInto : [ReleaseResult] -> String -> String
cacheTextInto [] suffix = suffix
cacheTextInto (result :: rest) suffix =
    case result of
        ReleaseFound repository _ _ _ etag =>
            cacheTextInto rest
                (if etag == "" then suffix else suffix <> repository <> "\t" <> etag <> "\n")
        | ReleaseUnchanged repository etag =>
            cacheTextInto rest (suffix <> repository <> "\t" <> etag <> "\n")
        | ReleaseFailed _ _ => cacheTextInto rest suffix

runRadar : String -> [String] -> Task ()
runRadar base repositories = do
    cache <- loadCache
    results <- runConcurrent base cache repositories
    printLine ("checked " <> show (listLength repositories) <> " repositories with a bounded 4-worker pool")
    printResults results
    std.io.writeFile cachePath (cacheText results)

emptyBytes : Bytes
emptyBytes = std.bytes.stringToBytes ""

findHeaderEnd : Bytes -> Int -> Maybe Int
findHeaderEnd bytes index =
    if index + 4 > std.bytes.bytesLength bytes then Nothing
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
        | Nothing => do
            chunk <- std.net.recvBytes connection 1024
            if std.bytes.bytesLength chunk == 0 then pureIO accumulated
            else readRequestHeaders connection (std.bytes.bytesAppend accumulated chunk)

fixtureBody : String
fixtureBody = "{\"tag_name\":\"v-local\",\"published_at\":\"2026-08-20\",\"html_url\":\"https://example.test/release\"}"

fixtureResponse : String
fixtureResponse =
    let body = fixtureBody in
    "HTTP/1.1 200 OK\u{d}\n"
        <> "Content-Type: application/json\u{d}\n"
        <> "ETag: local-etag\u{d}\n"
        <> "Content-Length: " <> show (std.bytes.bytesLength (std.bytes.stringToBytes body)) <> "\u{d}\n"
        <> "Connection: close\u{d}\n\u{d}\n"
        <> body

serveFixture : Socket -> Int -> Task ()
serveFixture listener total = do
    finished <- std.channel.newBoundedChan total
    acceptFixtures listener total finished
    awaitWorkers finished total
    std.channel.closeChan finished

acceptFixtures : Socket -> Int -> Chan () -> Task ()
acceptFixtures listener remaining finished =
    if remaining == 0 then pureIO ()
    else do
        connection <- std.net.accept listener
        thread <- std.thread.spawn
            (std.task.finally
                (catchError
                    (serveFixtureConnection connection)
                    (fun serverError => pureIO ()))
                (std.channel.send finished ()))
        acceptFixtures listener (remaining - 1) finished

serveFixtureConnection : Socket -> Task ()
serveFixtureConnection connection =
    std.task.finally
        (bindIO
            (readRequestHeaders connection emptyBytes)
            (sendFixtureResponse connection))
        (std.net.closeSocket connection)

sendFixtureResponse : Socket -> Bytes -> Task ()
sendFixtureResponse connection requestBytes =
    case std.bytes.bytesToString requestBytes of
        Err error => failIO error
        | Ok _ => do
            std.time.sleep 25
            std.net.sendBytes connection
                (std.bytes.stringToBytes fixtureResponse)

runLocal : [String] -> Task ()
runLocal repositories = do
    listenerInfo <- std.net.listen 0
    case listenerInfo of
        MkPair listener port =>
            std.task.finally
                (std.task.withThread
                    (serveFixture listener (listLength repositories))
                    (fun server => runRadar ("http://127.0.0.1:" <> show port) repositories))
                (std.net.closeSocket listener)

liveMode : [String] -> Bool
liveMode ("--live" :: _) = True
liveMode _ = False

main : Task ()
main = do
    repositoryText <- std.io.readFileLimited "repos.txt" 65536
    let repositories = splitLines repositoryText
    args <- getArgs
    if liveMode args then do
        printLine "release radar: live GitHub HTTPS"
        runRadar "https://api.github.com" repositories
    else do
        printLine "release radar: deterministic local service (pass --live for GitHub)"
        runLocal repositories
