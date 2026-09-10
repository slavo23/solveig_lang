import std.task(*)
import std.thread(*)
import std.channel(*)

spin : () -> Int
spin unit = spin unit

body : Task ()
body = bindIO (pureIO ()) (fun ignored => bindIO (pureIO (spin ())) (fun value => pureIO ()))

cleanup : Chan () -> Task ()
cleanup cleaned = do
    printLine "spinner-cleaned"
    send cleaned ()

worker : Chan () -> Task ()
worker cleaned = finally body (cleanup cleaned)

pause : Int -> Task ()
pause remaining =
    if remaining == 0
        then pureIO ()
        else do
                pureIO ()
                pause (remaining - 1)

main : Task ()
main = do
    cleaned <- newChan
    thread <- spawn (worker cleaned)
    pause 1000
    killThread thread
    _ <- recv cleaned
    printLine "spinner-killed"
