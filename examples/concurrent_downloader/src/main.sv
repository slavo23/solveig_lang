import std.net(*)
import std.bytes(*)
import std.task(*)
import std.channel(*)
import std.env(*)
import std.io(*)
import std.time(milliseconds)

data DownloadJob = DownloadJob String String

data DownloadResult = Downloaded Int | DownloadFailed

data DownloadStats = DownloadStats Int Int Int

emptyBytes : Bytes
emptyBytes = stringToBytes ""

maxResponseBytes : Int
maxResponseBytes = 1048576

netToHost : NetError -> HostError
netToHost (NetHostError _ hostError) = hostError
netToHost error = HostError InvalidArgument "network" (show error)

hostToNet : HostError -> NetError
hostToNet hostError = NetHostError "downloader" hostError

netTask : forall a. IO NetError a -> Task a
netTask = mapError netToHost

taskToNet : forall a. Task a -> IO NetError a
taskToNet = mapError hostToNet

utf8ToHost : Utf8Error -> HostError
utf8ToHost error = HostError InvalidArgument "bytesToString" (show error)

writeFileTask : String -> String -> Task ()
writeFileTask path contents = std.io.writeFile path contents

byteIs : Bytes -> Int -> Int -> Bool
byteIs bytes index expected =
    case bytesIndex bytes index of
        Just byte => byte == expected
        | Nothing => False

findHeaderEnd : Bytes -> Int -> Maybe Int
findHeaderEnd bytes index =
    if index + 4 > bytesLength bytes
        then Nothing
        else if and
                (byteIs bytes index 13)
                (and
                    (byteIs bytes (index + 1) 10)
                    (and (byteIs bytes (index + 2) 13) (byteIs bytes (index + 3) 10)))
            then Just (index + 4)
            else findHeaderEnd bytes (index + 1)

readHeaders : forall scope. TcpConnection scope -> Bytes -> Task Bytes
readHeaders socket accumulated =
    case findHeaderEnd accumulated 0 of
        Just end => pureIO accumulated
        | Nothing =>
            do
                chunk <- netTask (receiveTcpBytes socket 1024)
                if bytesLength chunk == 0
                    then failIO
                        (HostError
                            EndOfFile
                            "readHeaders"
                            "connection ended before the HTTP headers")
                    else readHeaders socket (bytesAppend accumulated chunk)

readResponse : forall scope. TcpConnection scope -> Bytes -> Task Bytes
readResponse socket accumulated = do
    chunk <- netTask (receiveTcpBytes socket 1024)
    if bytesLength chunk == 0
        then pureIO accumulated
        else if bytesLength accumulated + bytesLength chunk > maxResponseBytes
            then failIO
                (HostError
                    ResourceExhausted
                    "readResponse"
                    "HTTP response exceeded the one-megabyte limit")
            else readResponse socket (bytesAppend accumulated chunk)

makeRequest : String -> String -> String
makeRequest host path =
    appendString
        "GET "
        (appendString
            path
            (appendString
                " HTTP/1.0\u{d}\nHost: "
                (appendString host "\u{d}\nConnection: close\u{d}\n\u{d}\n")))

makeResponse : String -> String -> String
makeResponse status body =
    appendString
        "HTTP/1.0 "
        (appendString
            status
            (appendString
                "\u{d}\nContent-Length: "
                (appendString
                    (show (bytesLength (stringToBytes body)))
                    (appendString "\u{d}\nConnection: close\u{d}\n\u{d}\n" body))))

responseFor : String -> String
responseFor request =
    if request == makeRequest "localhost" "/alpha"
        then makeResponse "200 OK" "alpha"
        else if request == makeRequest "localhost" "/bravo"
            then makeResponse "200 OK" "bravo!"
            else if request == makeRequest "localhost" "/charlie"
                then makeResponse "200 OK" "charlie"
                else makeResponse "404 Not Found" "not found"

serveConnectionBody : forall scope. TcpConnection scope -> Task ()
serveConnectionBody connection = do
    requestBytes <- readHeaders connection emptyBytes
    case bytesToString requestBytes of
        Right request => netTask (sendTcpBytes connection (stringToBytes (responseFor request)))
        | Left error => failIO (utf8ToHost error)

serveConnection : forall scope. TcpConnection scope -> IO NetError ()
serveConnection connection = taskToNet (serveConnectionBody connection)

serve : forall scope. TcpListener scope -> Int -> Task ()
serve listener remaining =
    if remaining == 0
        then pureIO ()
        else do
            netTask (withAcceptedTcpConnection listener serveConnection)
            serve listener (remaining - 1)

successfulStatus : Bytes -> Bool
successfulStatus response =
    and (byteIs response 9 50) (and (byteIs response 10 48) (byteIs response 11 48))

responseBody : Bytes -> Either HostError Bytes
responseBody response =
    case findHeaderEnd response 0 of
        Just start => Right (bytesSlice response start (bytesLength response - start))
        | Nothing =>
            Left (HostError ProtocolError "responseBody" "HTTP response has no header terminator")

fetchWith : forall scope. String -> String -> TcpConnection scope -> Task Bytes
fetchWith host path socket = do
    netTask (sendTcpBytes socket (stringToBytes (makeRequest host path)))
    response <- readResponse socket emptyBytes
    if successfulStatus response
        then case responseBody response of
            Right body => pureIO body
            | Left error => failIO error
        else failIO (HostError ProtocolError "fetch" (appendString "HTTP request failed: " path))

fetch : String -> Port -> String -> String -> Task Bytes
fetch address port host path =
    netTask
        (withTcpConnection
            address
            port
            (fun connection => taskToNet (fetchWith host path connection)))

download :
    Chan DownloadResult ->
    (String -> Port -> String -> String -> Task (Maybe Bytes)) ->
    String -> Port -> String -> DownloadJob -> Task ()
download results fetcher address port host (DownloadJob path output) =
    catchError
        (downloadAttempt results fetcher address port host path output)
        (fun error => send results DownloadFailed)

fetchLocal : String -> Port -> String -> String -> Task (Maybe Bytes)
fetchLocal address port host path = do
    body <- fetch address port host path
    pureIO (Just body)

fetchInternet : String -> Port -> String -> String -> Task (Maybe Bytes)
fetchInternet address port host path = timeout (milliseconds 5000) (fetch address port host path)

downloadAttempt :
    Chan DownloadResult ->
    (String -> Port -> String -> String -> Task (Maybe Bytes)) ->
    String -> Port -> String -> String -> String -> Task ()
downloadAttempt results fetcher address port host path output = do
    writeFileTask output ""
    fetched <- fetcher address port host path
    case fetched of
        Just body =>
            case bytesToString body of
                Right contents =>
                    do
                        writeFileTask output contents
                        send results (Downloaded (bytesLength body))
                | Left error => failIO (utf8ToHost error)
        | Nothing =>
            failIO (HostError TimeExpired "download" "HTTP request timed out after five seconds")

worker :
    Chan DownloadJob ->
    Chan DownloadResult ->
    (String -> Port -> String -> String -> Task (Maybe Bytes)) ->
    String -> Port -> String -> Task ()
worker jobs results fetcher address port host =
    catchError (workerStep jobs results fetcher address port host) (fun error => pureIO ())

workerStep :
    Chan DownloadJob ->
    Chan DownloadResult ->
    (String -> Port -> String -> String -> Task (Maybe Bytes)) ->
    String -> Port -> String -> Task ()
workerStep jobs results fetcher address port host = do
    job <- recv jobs
    download results fetcher address port host job
    worker jobs results fetcher address port host

enqueueLocalJobs : Chan DownloadJob -> Task ()
enqueueLocalJobs jobs = do
    send jobs (DownloadJob "/alpha" "downloads/download-1.txt")
    send jobs (DownloadJob "/missing" "downloads/download-2.txt")
    send jobs (DownloadJob "/bravo" "downloads/download-3.txt")
    send jobs (DownloadJob "/charlie" "downloads/download-4.txt")
    closeChan jobs

enqueueInternetJobs : Chan DownloadJob -> Task ()
enqueueInternetJobs jobs = do
    send jobs (DownloadJob "/?solveig=1" "downloads/download-1.txt")
    send jobs (DownloadJob "/?solveig=2" "downloads/download-2.txt")
    send jobs (DownloadJob "/?solveig=3" "downloads/download-3.txt")
    send jobs (DownloadJob "/?solveig=4" "downloads/download-4.txt")
    closeChan jobs

collect : Chan DownloadResult -> Int -> Int -> Int -> Int -> Task DownloadStats
collect results remaining completed successes totalBytes =
    if remaining == 0
        then pureIO (DownloadStats successes (completed - successes) totalBytes)
        else do
            result <- recv results
            let nextCompleted = completed + 1
            let nextSuccesses =
                case result of
                    Downloaded size => successes + 1
                    | DownloadFailed => successes
            let nextBytes =
                case result of
                    Downloaded size => totalBytes + size
                    | DownloadFailed => totalBytes
            printLine (appendString "completed " (appendString (show nextCompleted) "/4"))
            collect results (remaining - 1) nextCompleted nextSuccesses nextBytes

printSummary : DownloadStats -> Task ()
printSummary (DownloadStats successes failures totalBytes) =
    printLine
        (appendString
            "summary: "
            (appendString
                (show successes)
                (appendString
                    " succeeded, "
                    (appendString
                        (show failures)
                        (appendString " failed, " (appendString (show totalBytes) " bytes"))))))

runPool :
    (String -> Port -> String -> String -> Task (Maybe Bytes)) ->
    String -> Port -> String -> (Chan DownloadJob -> Task ()) -> Task ()
runPool fetcher address port host enqueue = do
    jobs <- newBoundedChan 2
    results <- newBoundedChan 4
    withThread
        (worker jobs results fetcher address port host)
        (fun firstWorker =>
            withThread
                (worker jobs results fetcher address port host)
                (fun secondWorker =>
                    do
                        enqueue jobs
                        stats <- collect results 4 0 0 0
                        closeChan results
                        printSummary stats
                )
        )

runLocalExample : forall scope. String -> TcpListener scope -> Port -> Task ()
runLocalExample address listener port =
    withThread
        (serve listener 4)
        (fun _ => runPool fetchLocal address port "localhost" enqueueLocalJobs)

runLocalScoped : forall scope. TcpListener scope -> Port -> IO NetError ()
runLocalScoped listener port = taskToNet (runLocalExample "localhost" listener port)

runLocal : Task ()
runLocal = netTask (withTcpListener anyPort runLocalScoped)

runInternet : Task ()
runInternet = do
    printLine "downloading from http://neverssl.com with two workers"
    case portFromInt 80 of
        Left error => failIO (HostError InvalidArgument "portFromInt" (show error))
        | Right port => runPool fetchInternet "neverssl.com" port "neverssl.com" enqueueInternetJobs

useLocalServer : List String -> Bool
useLocalServer args =
    case args of
        Cons first rest => first == "--local"
        | Nil => False

main : Task ()
main = do
    printLine "downloader starting"
    args <- getArgs
    if useLocalServer args then runLocal else runInternet
