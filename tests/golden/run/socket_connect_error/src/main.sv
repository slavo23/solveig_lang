import std.net
reportConnect : IOError -> Task ()
reportConnect (ConnectError message) = printLine "connect-refused"
reportConnect _ = printLine "unexpected-error"

unexpectedConnect : Socket -> Task ()
unexpectedConnect connection = do
    closeSocket connection
    printLine "unexpected-connect"

connectAfterClose : Socket -> Int -> Task ()
connectAfterClose listener port = do
    closeSocket listener
    catchError
        (bindIO
            (connect "127.0.0.1" port)
            unexpectedConnect)
        reportConnect

main : Task ()
main = do
    listenerInfo <- listen 0
    case listenerInfo of
        MkPair listener port => connectAfterClose listener port
