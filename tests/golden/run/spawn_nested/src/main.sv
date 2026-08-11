pause : Int -> IO () ()
pause n =
    if n == 0 then pureIO ()
    else do
        pureIO ()
        pause (n - 1)

parent : () -> IO () ()
parent _ = do
    _ <- spawn (printLine "nested")
    pause 10000
    printLine "parent"

main : IO () ()
main = do
    _ <- spawn (parent ())
    pause 30000
    printLine "main"
