import std.bytes(*)
import std.net(*)
import std.stream(fold)
import std.task(*)

netToIO : NetError -> IOError
netToIO (NetIOError _ ioError) = ioError
netToIO error = IOError InvalidArgument "network" (show error)

netTask : forall a. IO NetError a -> Task a
netTask = mapError netToIO

echoConnection : forall scope. TcpConnection scope -> IO NetError ()
echoConnection connection = do
    request <- receiveTcpBytes connection 1024
    sendTcpBytes connection request

serveOnce : forall scope. TcpListener scope -> IO NetError ()
serveOnce listener = withAcceptedTcpConnection listener echoConnection

runClient : Port -> IO NetError String
runClient port =
    withTcpConnection
        "localhost"
        port
        (fun connection => do
            sendTcpBytes connection (stringToBytes "echo over TCP")
            reply <- fold bytesAppend (stringToBytes "") (tcpBytes connection 4)
            case bytesToString reply of
                Right text => pureIO text
                | Left _ => pureIO "invalid reply"
        )

runFixture : forall scope. TcpListener scope -> Port -> IO NetError String
runFixture listener port = withThread (serveOnce listener) (fun _ => runClient port)

main : Task ()
main = do
    reply <- netTask (withTcpListener anyPort runFixture)
    printLine reply
