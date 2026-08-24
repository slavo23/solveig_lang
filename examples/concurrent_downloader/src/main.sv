import std.net
import std.bytes
import std.task
import std.channel
import std.env
import std.io

data DownloadJob = DownloadJob String String

data DownloadResult = Downloaded Int | DownloadFailed

data DownloadStats = DownloadStats Int Int Int

emptyBytes : Bytes
emptyBytes = stringToBytes ""

maxResponseBytes : Int
maxResponseBytes = 1048576

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

readHeaders : Socket -> Bytes -> Task Bytes
readHeaders socket accumulated =
    case findHeaderEnd accumulated 0 of
        Just end => pureIO accumulated
        | Nothing =>
            do
                chunk <- recvBytes socket 1024
                if bytesLength chunk == 0
                    then failIO (RecvError "connection ended before the HTTP headers")
                    else readHeaders socket (bytesAppend accumulated chunk)

readResponse : Socket -> Bytes -> Task Bytes
readResponse socket accumulated = do
    chunk <- recvBytes socket 1024
    if bytesLength chunk == 0
        then pureIO accumulated
        else if bytesLength accumulated + bytesLength chunk > maxResponseBytes
            then failIO (RecvError "HTTP response exceeded the one-megabyte limit")
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

serveConnection : Socket -> Task ()
serveConnection connection = finally (serveConnectionBody connection) (closeSocket connection)

serveConnectionBody : Socket -> Task ()
serveConnectionBody connection = do
    requestBytes <- readHeaders connection emptyBytes
    case bytesToString requestBytes of
        Ok request => sendBytes connection (stringToBytes (responseFor request))
        | Err error => failIO error

serve : Socket -> Int -> Task ()
serve listener remaining =
    if remaining == 0
        then pureIO ()
        else do
            connection <- accept listener
            serveConnection connection
            serve listener (remaining - 1)

successfulStatus : Bytes -> Bool
successfulStatus response =
    and (byteIs response 9 50) (and (byteIs response 10 48) (byteIs response 11 48))

responseBody : Bytes -> Result IOError Bytes
responseBody response =
    case findHeaderEnd response 0 of
        Just start => Ok (bytesSlice response start (bytesLength response - start))
        | Nothing => Err (RecvError "HTTP response has no header terminator")

fetchWith : String -> String -> Socket -> Task Bytes
fetchWith host path socket = do
    sendBytes socket (stringToBytes (makeRequest host path))
    response <- readResponse socket emptyBytes
    if successfulStatus response
        then case responseBody response of
            Ok body => pureIO body
            | Err error => failIO error
        else failIO (RecvError (appendString "HTTP request failed: " path))

fetch : String -> Int -> String -> String -> Task Bytes
fetch address port host path = bracket (connect address port) closeSocket (fetchWith host path)

download :
    Chan DownloadResult ->
    (String -> Int -> String -> String -> Task (Maybe Bytes)) ->
    String -> Int -> String -> DownloadJob -> Task ()
download results fetcher address port host (DownloadJob path output) =
    catchError
        (downloadAttempt results fetcher address port host path output)
        (fun error => send results DownloadFailed)

fetchLocal : String -> Int -> String -> String -> Task (Maybe Bytes)
fetchLocal address port host path = do
    body <- fetch address port host path
    pureIO (Just body)

fetchInternet : String -> Int -> String -> String -> Task (Maybe Bytes)
fetchInternet address port host path = timeout 5000 (fetch address port host path)

downloadAttempt :
    Chan DownloadResult ->
    (String -> Int -> String -> String -> Task (Maybe Bytes)) ->
    String -> Int -> String -> String -> String -> Task ()
downloadAttempt results fetcher address port host path output = do
    writeFile output ""
    fetched <- fetcher address port host path
    case fetched of
        Just body =>
            case bytesToString body of
                Ok contents =>
                    do
                        writeFile output contents
                        send results (Downloaded (bytesLength body))
                | Err error => failIO error
        | Nothing => failIO (RecvError "HTTP request timed out after five seconds")

worker :
    Chan DownloadJob ->
    Chan DownloadResult ->
    (String -> Int -> String -> String -> Task (Maybe Bytes)) -> String -> Int -> String -> Task ()
worker jobs results fetcher address port host =
    catchError (workerStep jobs results fetcher address port host) (fun error => pureIO ())

workerStep :
    Chan DownloadJob ->
    Chan DownloadResult ->
    (String -> Int -> String -> String -> Task (Maybe Bytes)) -> String -> Int -> String -> Task ()
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
    (String -> Int -> String -> String -> Task (Maybe Bytes)) ->
    String -> Int -> String -> (Chan DownloadJob -> Task ()) -> Task ()
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

runLocalExample : String -> Socket -> Int -> Task ()
runLocalExample address listener port =
    finally
        (withThread
            (serve listener 4)
            (fun server => runPool fetchLocal address port "localhost" enqueueLocalJobs))
        (closeSocket listener)

runLocal : Task ()
runLocal = do
    address <- resolveHost "localhost"
    listenerInfo <- listen 0
    case listenerInfo of
        (listener, port) => runLocalExample address listener port

runInternet : Task ()
runInternet = do
    printLine "downloading from http://neverssl.com with two workers"
    address <- resolveHost "neverssl.com"
    runPool fetchInternet address 80 "neverssl.com" enqueueInternetJobs

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
