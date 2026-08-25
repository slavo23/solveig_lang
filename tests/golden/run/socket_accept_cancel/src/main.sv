import std.net(*)
import std.task(*)

netToHost : NetError -> HostError
netToHost (NetHostError _ hostError) = hostError

netTask : forall a. IO NetError a -> Task a
netTask = mapError netToHost

waitForClient : Socket -> Task ()
waitForClient listener = do
    connection <- netTask (accept listener)
    netTask (closeSocket connection)

finishScope : ThreadId -> Task ()
finishScope thread = printLine "scope-body"

main : Task ()
main = do
    listenerInfo <- netTask (listen anyPort)
    case listenerInfo of
        (listener, port) => do
            withThread (waitForClient listener) finishScope
            netTask (closeSocket listener)
            printLine "accept-cancelled"
