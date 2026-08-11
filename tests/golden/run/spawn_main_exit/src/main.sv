main : IO () ()
main = do
    _ <- spawn (printLine "child")
    printLine "main"
