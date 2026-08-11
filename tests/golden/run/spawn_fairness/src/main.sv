spin : Int -> ()
spin n = if n == 0 then () else spin (n - 1)

pause : Int -> IO () ()
pause n =
    if n == 0 then pureIO ()
    else do
        pureIO ()
        pause (n - 1)

spinner : () -> IO () ()
spinner _ = do
    pureIO ()
    _ <- pureIO (spin 100000)
    printLine "spinner"

main : IO () ()
main = do
    _ <- spawn (spinner ())
    _ <- spawn (printLine "other")
    pause 30000
    printLine "main"
