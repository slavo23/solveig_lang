import std.net(*)

netToHost : NetError -> HostError
netToHost (NetHostError _ hostError) = hostError
netToHost error = HostError InvalidArgument "network" (show error)

rememberPort : forall scope. TcpListener scope -> Port -> IO NetError Port
rememberPort _ port = pureIO port

unexpectedConnect : forall scope. TcpConnection scope -> IO NetError ()
unexpectedConnect _ = pureIO ()

reportConnect : HostError -> Task ()
reportConnect (HostError _ _ _) = printLine "connect-refused"

main : Task ()
main = do
    port <- mapError netToHost (withTcpListener anyPort rememberPort)
    catchError
        (bindIO
            (mapError netToHost (withTcpConnection "localhost" port unexpectedConnect))
            (fun _ => printLine "unexpected-connect"))
        reportConnect
