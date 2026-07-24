main : IO ()
main = do
    args <- getArgs
    mapIO_ printLine args
