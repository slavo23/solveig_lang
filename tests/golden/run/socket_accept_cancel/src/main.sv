import std.net
import std.task
waitForClient : Socket -> Task ()
waitForClient listener = do
    connection <- accept listener
    closeSocket connection

finishScope : ThreadId -> Task ()
finishScope thread = printLine "scope-body"

main : Task ()
main = do
    listenerInfo <- listen 0
    case listenerInfo of
        MkPair listener port => do
            withThread (waitForClient listener) finishScope
            closeSocket listener
            printLine "accept-cancelled"
