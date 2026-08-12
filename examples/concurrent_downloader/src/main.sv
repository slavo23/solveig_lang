data DownloadJob = DownloadJob String

data DownloadResult =
    Downloaded Int
  | DownloadFailed

data DownloadStats = DownloadStats Int Int Int

emptyBytes : Bytes
emptyBytes = stringToBytes ""

byteIs : Bytes -> Int -> Int -> Bool
byteIs bytes index expected =
    case bytesIndex bytes index of
        Just byte => byte == expected
        | Nothing => False

findHeaderEnd : Bytes -> Int -> Maybe Int
findHeaderEnd bytes index =
    if index + 4 > bytesLength bytes then
        Nothing
    else if
        and
            (byteIs bytes index 13)
            (and
                (byteIs bytes (index + 1) 10)
                (and
                    (byteIs bytes (index + 2) 13)
                    (byteIs bytes (index + 3) 10)))
    then
        Just (index + 4)
    else
        findHeaderEnd bytes (index + 1)

readHeaders : Socket -> Bytes -> Task Bytes
readHeaders socket accumulated =
    case findHeaderEnd accumulated 0 of
        Just end => pureIO accumulated
        | Nothing => do
            chunk <- recvBytes socket 1024
            if bytesLength chunk == 0 then
                failIO (RecvError "connection ended before the HTTP headers")
            else
                readHeaders socket (bytesAppend accumulated chunk)

readResponse : Socket -> Bytes -> Task Bytes
readResponse socket accumulated = do
    chunk <- recvBytes socket 1024
    if bytesLength chunk == 0 then
        pureIO accumulated
    else
        readResponse socket (bytesAppend accumulated chunk)

makeRequest : String -> String
makeRequest path =
    appendString
        "GET "
        (appendString
            path
            " HTTP/1.0\u{d}\nHost: localhost\u{d}\nConnection: close\u{d}\n\u{d}\n")

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
    if request == makeRequest "/alpha" then
        makeResponse "200 OK" "alpha"
    else if request == makeRequest "/bravo" then
        makeResponse "200 OK" "bravo!"
    else if request == makeRequest "/charlie" then
        makeResponse "200 OK" "charlie"
    else
        makeResponse "404 Not Found" "not found"

serveConnection : Socket -> Task ()
serveConnection connection =
    finally
        (serveConnectionBody connection)
        (closeSocket connection)

serveConnectionBody : Socket -> Task ()
serveConnectionBody connection = do
    requestBytes <- readHeaders connection emptyBytes
    case bytesToString requestBytes of
        Ok request => sendBytes connection (stringToBytes (responseFor request))
        | Err error => failIO error

serve : Socket -> Int -> Task ()
serve listener remaining =
    if remaining == 0 then
        pureIO ()
    else do
        connection <- accept listener
        serveConnection connection
        serve listener (remaining - 1)

successfulStatus : Bytes -> Bool
successfulStatus response =
    and
        (byteIs response 9 50)
        (and (byteIs response 10 48) (byteIs response 11 48))

responseBody : Bytes -> Result IOError Bytes
responseBody response =
    case findHeaderEnd response 0 of
        Just start =>
            Ok (bytesSlice response start (bytesLength response - start))
        | Nothing => Err (RecvError "HTTP response has no header terminator")

fetchWith : String -> Socket -> Task Bytes
fetchWith path socket = do
    sendBytes socket (stringToBytes (makeRequest path))
    response <- readResponse socket emptyBytes
    if successfulStatus response then
        case responseBody response of
            Ok body => pureIO body
            | Err error => failIO error
    else
        failIO (RecvError (appendString "HTTP request failed: " path))

fetch : Int -> String -> Task Bytes
fetch port path =
    bracket
        (connect "127.0.0.1" port)
        closeSocket
        (fetchWith path)

download : Chan DownloadResult -> Int -> DownloadJob -> Task ()
download results port (DownloadJob path) =
    catchError
        (downloadAttempt results port path)
        (fun error => send results DownloadFailed)

downloadAttempt : Chan DownloadResult -> Int -> String -> Task ()
downloadAttempt results port path = do
    body <- fetch port path
    send results (Downloaded (bytesLength body))

worker : Chan DownloadJob -> Chan DownloadResult -> Int -> Task ()
worker jobs results port =
    catchError
        (workerStep jobs results port)
        (fun error => pureIO ())

workerStep : Chan DownloadJob -> Chan DownloadResult -> Int -> Task ()
workerStep jobs results port = do
    job <- recv jobs
    download results port job
    worker jobs results port

enqueueJobs : Chan DownloadJob -> Task ()
enqueueJobs jobs = do
    send jobs (DownloadJob "/alpha")
    send jobs (DownloadJob "/missing")
    send jobs (DownloadJob "/bravo")
    send jobs (DownloadJob "/charlie")
    closeChan jobs

collect : Chan DownloadResult -> Int -> Int -> Int -> Int -> Task DownloadStats
collect results remaining completed successes totalBytes =
    if remaining == 0 then
        pureIO (DownloadStats successes (completed - successes) totalBytes)
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
        printLine
            (appendString
                "completed "
                (appendString (show nextCompleted) "/4"))
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
                        (appendString
                            " failed, "
                            (appendString (show totalBytes) " bytes"))))))

runPool : Int -> Task ()
runPool port = do
    jobs <- newBoundedChan 2
    results <- newBoundedChan 4
    withThread
        (worker jobs results port)
        (fun firstWorker =>
            withThread
                (worker jobs results port)
                (fun secondWorker => do
                    enqueueJobs jobs
                    stats <- collect results 4 0 0 0
                    closeChan results
                    printSummary stats))

runExample : Socket -> Int -> Task ()
runExample listener port =
    finally
        (withThread
            (serve listener 4)
            (fun server => runPool port))
        (closeSocket listener)

main : Task ()
main = do
    printLine "downloader starting"
    listenerInfo <- listen 0
    case listenerInfo of
        MkPair listener port => runExample listener port
