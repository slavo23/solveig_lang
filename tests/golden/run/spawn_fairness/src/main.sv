import std.thread(*)

spin : Int -> ()
spin n = if n == 0 then () else spin (n - 1)

pause : Int -> Task ()
pause n =
    if n == 0
        then pureIO ()
        else do
                pureIO ()
                pause (n - 1)

spinner : () -> Task ()
spinner _ = do
    pureIO ()
    _ <- pureIO (spin 100000)
    printLine "spinner"

main : Task ()
main = do
    _ <- spawn (spinner ())
    _ <- spawn (printLine "other")
    pause 30000
    printLine "main"
