import std.task(*)
import std.thread(*)
import std.channel(*)

pause : Int -> Task ()
pause remaining =
    if remaining == 0 then pureIO ()
    else do
        pureIO ()
        pause (remaining - 1)

maskedBody : Chan () -> Chan () -> Task ()
maskedBody ready finished = do
    send ready ()
    pause 1000
    send finished ()

worker : Chan () -> Chan () -> Chan () -> Task ()
worker ready finished cleaned =
    finally (maskedThenUnexpected ready finished) (send cleaned ())

maskedThenUnexpected : Chan () -> Chan () -> Task ()
maskedThenUnexpected ready finished = do
    mask (maskedBody ready finished)
    pureIO ()

main : Task ()
main = do
    ready <- newChan
    finished <- newChan
    cleaned <- newChan
    thread <- spawn (worker ready finished cleaned)
    _ <- recv ready
    killThread thread
    _ <- recv cleaned
    _ <- recv finished
    printLine "mask-deferred"
