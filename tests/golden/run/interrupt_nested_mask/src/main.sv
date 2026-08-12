pause : Int -> Task ()
pause remaining =
    if remaining == 0 then pureIO ()
    else do
        pureIO ()
        pause (remaining - 1)

inner : Chan () -> Task ()
inner ready = do
    send ready ()
    pause 1000

nested : Chan () -> Chan () -> Task ()
nested ready innerFinished =
    mask
        (do
            mask (inner ready)
            send innerFinished ()
            pause 1000)

worker : Chan () -> Chan () -> Chan () -> Task ()
worker ready innerFinished cleaned =
    finally (nested ready innerFinished) (send cleaned ())

main : Task ()
main = do
    ready <- newChan
    innerFinished <- newChan
    cleaned <- newChan
    thread <- spawn (worker ready innerFinished cleaned)
    _ <- recv ready
    killThread thread
    _ <- recv cleaned
    _ <- recv innerFinished
    printLine "outer-interrupted"
