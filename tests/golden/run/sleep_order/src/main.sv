import std.time(*)
import std.thread(*)
import std.channel(*)

worker : Int -> String -> Chan () -> Task ()
worker delay label done = do
    sleep (milliseconds delay)
    printLine label
    send done ()

main : Task ()
main = do
    done <- newChan
    _ <- spawn (worker 30 "thirty" done)
    _ <- spawn (worker 10 "ten" done)
    _ <- spawn (worker 20 "twenty" done)
    _ <- recv done
    _ <- recv done
    _ <- recv done
    pureIO ()
