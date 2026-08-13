import std.task
import std.thread
import std.channel
waitMasked : Chan Int -> Chan () -> Task ()
waitMasked values ready =
    mask
        (do
            send ready ()
            _ <- recv values
            printLine "unexpected receive")

worker : Chan Int -> Chan () -> Chan () -> Task ()
worker values ready cleaned =
    finally (waitMasked values ready) (send cleaned ())

main : Task ()
main = do
    values <- newChan
    ready <- newChan
    cleaned <- newChan
    thread <- spawn (worker values ready cleaned)
    _ <- recv ready
    killThread thread
    _ <- recv cleaned
    printLine "masked-recv-interrupted"
