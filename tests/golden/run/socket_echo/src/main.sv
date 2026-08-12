echoOnce : Socket -> Task ()
echoOnce listener = do
    connection <- accept listener
    request <- recvBytes connection 1024
    sendBytes connection request
    closeSocket connection

runClient : Socket -> Int -> Task ()
runClient listener port = do
    connection <- connect "127.0.0.1" port
    sendBytes connection (stringToBytes "echo over TCP")
    reply <- recvBytes connection 1024
    closeSocket connection
    closeSocket listener
    case bytesToString reply of
        Ok text => printLine text
        | Err error => printLine "invalid reply"

main : Task ()
main = do
    listenerInfo <- listen 0
    case listenerInfo of
        MkPair listener port =>
            withThread
                (echoOnce listener)
                (fun thread => runClient listener port)
