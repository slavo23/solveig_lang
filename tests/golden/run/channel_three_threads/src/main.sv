import std.thread
import std.channel
worker : Chan Int -> Chan () -> Chan String -> String -> Task ()
worker channel ready replies label = do
    send ready ()
    value <- recv channel
    send replies (appendString label (show value))

main : Task ()
main = do
    channel <- newChan
    ready <- newChan
    replies <- newChan
    _ <- spawn (worker channel ready replies "A")
    _ <- spawn (worker channel ready replies "B")
    _ <- spawn (worker channel ready replies "C")
    _ <- recv ready
    _ <- recv ready
    _ <- recv ready
    send channel 1
    send channel 2
    send channel 3
    first <- recv replies
    second <- recv replies
    third <- recv replies
    printLine first
    printLine second
    printLine third
