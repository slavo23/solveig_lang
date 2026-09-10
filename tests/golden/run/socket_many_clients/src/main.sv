import std.bytes(*)
import std.channel(*)
import std.net(*)
import std.task(*)
import std.thread(*)

netToHost : NetError -> HostError
netToHost (NetHostError _ hostError) = hostError
netToHost error = HostError InvalidArgument "network" (show error)

hostToNet : String -> HostError -> NetError
hostToNet context hostError = NetHostError context hostError

serveConnection : forall scope. TcpConnection scope -> IO NetError ()
serveConnection connection = do
    request <- receiveTcpBytes connection 64
    sendTcpBytes connection request

serveClients : forall scope. TcpListener scope -> Int -> IO NetError ()
serveClients listener remaining =
    if remaining == 0
        then pureIO ()
        else do
                withAcceptedTcpConnection listener serveConnection
                serveClients listener (remaining - 1)

runClient : forall scope. TcpConnection scope -> IO NetError Int
runClient connection = do
    sendTcpBytes connection (stringToBytes "ping")
    reply <- receiveTcpBytes connection 64
    case bytesToString reply of
        Right _ => pureIO 1
        | Left _ => pureIO 0

client : Chan Int -> Port -> Int -> IO NetError ()
client results port _ = do
    completed <- withTcpConnection "localhost" port runClient
    mapError (hostToNet "result channel") (send results completed)

spawnClients : Chan Int -> Port -> Int -> IO NetError ()
spawnClients results port remaining =
    if remaining == 0
        then pureIO ()
        else do
                _ <- spawn (client results port remaining)
                spawnClients results port (remaining - 1)

collectClients : Chan Int -> Int -> Int -> IO NetError Int
collectClients results remaining total =
    if remaining == 0
        then pureIO total
        else do
                completed <- mapError (hostToNet "result channel") (recv results)
                collectClients results (remaining - 1) (total + completed)

runClients : forall scope. TcpListener scope -> Port -> IO NetError ()
runClients listener port = do
    results <- mapError (hostToNet "result channel") (newBoundedChan 16)
    spawnClients results port 16
    completed <- collectClients results 16 0
    mapError (hostToNet "stdout") (printLine (show completed))

runFixture : forall scope. TcpListener scope -> Port -> IO NetError ()
runFixture listener port = withThread (serveClients listener 16) (fun _ => runClients listener port)

main : Task ()
main = mapError netToHost (withTcpListener anyPort runFixture)
