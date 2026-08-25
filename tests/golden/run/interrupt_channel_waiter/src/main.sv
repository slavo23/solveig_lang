import std.task(*)
import std.thread(*)
import std.channel(*)

waitForValue : Chan Int -> Chan () -> Task ()
waitForValue values ready = do
    send ready ()
    _ <- recv values
    printLine "unexpected receive"

worker : Chan Int -> Chan () -> Chan () -> Task ()
worker values ready cleaned =
    finally (waitForValue values ready) (send cleaned ())

main : Task ()
main = do
    values <- newChan
    ready <- newChan
    cleaned <- newChan
    thread <- spawn (worker values ready cleaned)
    _ <- recv ready
    killThread thread
    _ <- recv cleaned
    send values 42
    value <- recv values
    printLine (show value)
