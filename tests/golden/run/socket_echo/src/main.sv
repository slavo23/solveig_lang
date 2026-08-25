import std.net(*)
import std.bytes(*)
import std.task(*)

echoConnection : Socket -> Task ()
echoConnection connection = do
    request <- recvBytes connection 1024
    sendBytes connection request

runClient : Int -> Task ()
runClient port =
    withConnection "127.0.0.1" port (fun connection => do
        sendBytes connection (stringToBytes "echo over TCP")
        reply <- recvBytes connection 1024
        case bytesToString reply of
            Right text => printLine text
            | Left error => printLine "invalid reply")

main : Task ()
main =
    withListener 0 (fun listenerInfo =>
        case listenerInfo of
            (listener, port) =>
                withThread
                    (withAcceptedConnection listener echoConnection)
                    (fun thread => runClient port))
