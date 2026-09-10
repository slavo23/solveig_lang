import std.thread(*)
import std.channel(*)

pause : Int -> Task ()
pause n =
    if n == 0
        then pureIO ()
        else do
                pureIO ()
                pause (n - 1)

worker : Chan () -> String -> Task ()
worker done label = do
    printLine (appendString label "1")
    pause 10000
    printLine (appendString label "2")
    send done ()

main : Task ()
main = do
    done <- newChan
    _ <- spawn (worker done "A")
    _ <- spawn (worker done "B")
    _ <- recv done
    _ <- recv done
    printLine "main"
