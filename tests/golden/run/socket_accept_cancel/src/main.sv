import std.net(*)
import std.task(*)

netToIO : NetError -> IOError
netToIO (NetIOError _ ioError) = ioError
netToIO error = IOError InvalidArgument "network" (show error)

waitForClient : forall scope. TcpListener scope -> IO NetError ()
waitForClient listener = withAcceptedTcpConnection listener (fun _ => pureIO ())

finishScope : ThreadId -> IO NetError ()
finishScope _ = pureIO ()

cancelAccept : forall scope. TcpListener scope -> Port -> IO NetError ()
cancelAccept listener _ = withThread (waitForClient listener) finishScope

main : Task ()
main = do
    mapError netToIO (withTcpListener anyPort cancelAccept)
    printLine "accept-cancelled"
