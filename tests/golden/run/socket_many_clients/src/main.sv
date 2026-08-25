import std.net(*)
import std.bytes(*)
import std.task(*)
import std.thread(*)
import std.channel(*)

netToHost : NetError -> HostError
netToHost (NetHostError _ hostError) = hostError

netTask : forall a. IO NetError a -> Task a
netTask = mapError netToHost

serveClients : Socket -> Int -> Task ()
serveClients listener remaining =
    if remaining == 0 then
        pureIO ()
    else do
        connection <- netTask (accept listener)
        request <- netTask (recvBytes connection 64)
        netTask (sendBytes connection request)
        netTask (closeSocket connection)
        serveClients listener (remaining - 1)

client : Chan Int -> Port -> Int -> Task ()
client results port number = do
    connection <- netTask (connect "127.0.0.1" port)
    netTask (sendBytes connection (stringToBytes "ping"))
    reply <- netTask (recvBytes connection 64)
    netTask (closeSocket connection)
    case bytesToString reply of
        Right text => send results 1
        | Left error => send results 0

spawnClients : Chan Int -> Port -> Int -> Task ()
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

runClients : Socket -> Port -> Task ()
runClients listener port = do
    results <- newBoundedChan 16
    spawnClients results port 16
    completed <- collectClients results 16 0
    netTask (closeSocket listener)
    printLine (show completed)

main : Task ()
main = do
    listenerInfo <- netTask (listen anyPort)
    case listenerInfo of
        (listener, port) =>
            withThread
                (serveClients listener 16)
                (fun thread => runClients listener port)
