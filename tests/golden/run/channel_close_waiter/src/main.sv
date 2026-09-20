import std.thread(*)
import std.channel(*)

closedRecv : IOError -> Task Int
closedRecv (IOError ResourceVanished _ _) = pureIO 42
closedRecv _ = pureIO (-1)

waiter : Chan Int -> Chan () -> Chan Int -> Task ()
waiter channel ready result = do
    send ready ()
    value <- catchError (recv channel) closedRecv
    send result value

main : Task ()
main = do
    channel <- newChan
    ready <- newChan
    result <- newChan
    _ <- spawn (waiter channel ready result)
    _ <- recv ready
    closeChan channel
    value <- recv result
    printLine (show value)
