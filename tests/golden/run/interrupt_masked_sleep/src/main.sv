import std.task(*)
import std.time(*)
import std.thread(*)
import std.channel(*)

wait : Chan () -> Task ()
wait ready = do
    send ready ()
    mask (sleep 1000000000)

cleanup : Chan () -> Task ()
cleanup cleaned = do
    printLine "sleep-cleaned"
    send cleaned ()

worker : Chan () -> Chan () -> Task ()
worker ready cleaned = finally (wait ready) (cleanup cleaned)

main : Task ()
main = do
    ready <- newChan
    cleaned <- newChan
    thread <- spawn (worker ready cleaned)
    _ <- recv ready
    killThread thread
    _ <- recv cleaned
    printLine "sleep-killed"
