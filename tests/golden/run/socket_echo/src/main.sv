import std.net(*)
import std.bytes(*)
import std.task(*)

netToHost : NetError -> HostError
netToHost (NetHostError _ hostError) = hostError

netTask : forall a. IO NetError a -> Task a
netTask = mapError netToHost

echoConnection : Socket -> Task ()
echoConnection connection = do
    request <- netTask (recvBytes connection 1024)
    netTask (sendBytes connection request)

runClient : Int -> Task ()
runClient port =
    bracket
        (netTask (connect "127.0.0.1" port))
        (netTask . closeSocket)
        (fun connection => do
            netTask (sendBytes connection (stringToBytes "echo over TCP"))
            reply <- netTask (recvBytes connection 1024)
            case bytesToString reply of
                Right text => printLine text
                | Left error => printLine "invalid reply")

main : Task ()
main =
    bracket (netTask (listen 0)) (fun (listener, _) => netTask (closeSocket listener)) (fun listenerInfo =>
        case listenerInfo of
            (listener, port) =>
                withThread
                    (bracket (netTask (accept listener)) (netTask . closeSocket) echoConnection)
                    (fun thread => runClient port))
