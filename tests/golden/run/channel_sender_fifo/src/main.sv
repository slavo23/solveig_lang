import std.thread(*)
import std.channel(*)

sender : Chan Int -> Chan () -> Int -> Task ()
sender values ready value = do
    send ready ()
    send values value

main : Task ()
main = do
    values <- newBoundedChan 1
    ready <- newChan
    send values 0
    _ <- spawn (sender values ready 1)
    _ <- spawn (sender values ready 2)
    _ <- spawn (sender values ready 3)
    _ <- recv ready
    _ <- recv ready
    _ <- recv ready
    first <- recv values
    second <- recv values
    third <- recv values
    fourth <- recv values
    printLine (show first)
    printLine (show second)
    printLine (show third)
    printLine (show fourth)
