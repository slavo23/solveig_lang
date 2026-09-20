import std.net(*)

netToIO : NetError -> IOError
netToIO (NetIOError _ ioError) = ioError
netToIO error = IOError InvalidArgument "network" (show error)

rememberPort : forall scope. TcpListener scope -> Port -> IO NetError Port
rememberPort _ port = pureIO port

unexpectedConnect : forall scope. TcpConnection scope -> IO NetError ()
unexpectedConnect _ = pureIO ()

reportConnect : IOError -> Task ()
reportConnect (IOError _ _ _) = printLine "connect-refused"

main : Task ()
main = do
    port <- mapError netToIO (withTcpListener anyPort rememberPort)
    catchError
        (bindIO
            (mapError netToIO (withTcpConnection "localhost" port unexpectedConnect))
            (fun _ => printLine "unexpected-connect"))
        reportConnect
