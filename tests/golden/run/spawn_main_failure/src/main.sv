main : IO ()
main = do
    _ <- pureIO (div 1 0)
    pureIO ()
