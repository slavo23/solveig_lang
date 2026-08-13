import std.thread
import std.channel
reportClosed : Chan String -> IOError -> Task ()
reportClosed result ChannelClosed = send result "sender-closed"
reportClosed result _ = send result "unexpected-error"

sender : Chan Int -> Chan () -> Chan String -> Task ()
sender values ready result = do
    send ready ()
    catchError (send values 2) (reportClosed result)

main : Task ()
main = do
    values <- newBoundedChan 1
    ready <- newChan
    result <- newChan
    send values 1
    _ <- spawn (sender values ready result)
    _ <- recv ready
    closeChan values
    message <- recv result
    printLine message
