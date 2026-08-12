serveClients : Socket -> Int -> Task ()
serveClients listener remaining =
    if remaining == 0 then
        pureIO ()
    else do
        connection <- accept listener
        request <- recvBytes connection 64
        sendBytes connection request
        closeSocket connection
        serveClients listener (remaining - 1)

client : Chan Int -> Int -> Int -> Task ()
client results port number = do
    connection <- connect "127.0.0.1" port
    sendBytes connection (stringToBytes "ping")
    reply <- recvBytes connection 64
    closeSocket connection
    case bytesToString reply of
        Ok text => send results 1
        | Err error => send results 0

spawnClients : Chan Int -> Int -> Int -> Task ()
spawnClients results port remaining =
    if remaining == 0 then
        pureIO ()
    else do
        _ <- spawn (client results port remaining)
        spawnClients results port (remaining - 1)

collectClients : Chan Int -> Int -> Int -> Task Int
collectClients results remaining total =
    if remaining == 0 then
        pureIO total
    else do
        completed <- recv results
        collectClients results (remaining - 1) (total + completed)

runClients : Socket -> Int -> Task ()
runClients listener port = do
    results <- newBoundedChan 16
    spawnClients results port 16
    completed <- collectClients results 16 0
    closeSocket listener
    printLine (show completed)

main : Task ()
main = do
    listenerInfo <- listen 0
    case listenerInfo of
        MkPair listener port =>
            withThread
                (serveClients listener 16)
                (fun thread => runClients listener port)
