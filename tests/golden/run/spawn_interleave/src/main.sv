import std.thread
pause : Int -> Task ()
pause n =
    if n == 0 then pureIO ()
    else do
        pureIO ()
        pause (n - 1)

worker : String -> Task ()
worker label = do
    printLine (appendString label "1")
    pause 10000
    printLine (appendString label "2")

main : Task ()
main = do
    _ <- spawn (worker "A")
    _ <- spawn (worker "B")
    pause 30000
    printLine "main"
