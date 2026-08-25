import std.net(*)

netToHost : NetError -> HostError
netToHost (NetHostError _ hostError) = hostError

connectTask : String -> Port -> Task Socket
connectTask host port = mapError netToHost (connect host port)

listenTask : Port -> Task (Socket, Port)
listenTask port = mapError netToHost (listen port)

closeSocketTask : Socket -> Task ()
closeSocketTask socket = mapError netToHost (closeSocket socket)

reportConnect : HostError -> Task ()
reportConnect (HostError _ _ _) = printLine "connect-refused"
reportConnect _ = printLine "unexpected-error"

unexpectedConnect : Socket -> Task ()
unexpectedConnect connection = do
    closeSocketTask connection
    printLine "unexpected-connect"

connectAfterClose : Socket -> Port -> Task ()
connectAfterClose listener port = do
    closeSocketTask listener
    catchError
        (bindIO
            (connectTask "127.0.0.1" port)
            unexpectedConnect)
        reportConnect

main : Task ()
main = do
    listenerInfo <- listenTask anyPort
    case listenerInfo of
        (listener, port) => connectAfterClose listener port
