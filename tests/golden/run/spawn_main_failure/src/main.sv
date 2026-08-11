main : Task ()
main = do
    _ <- pureIO (div 1 0)
    pureIO ()
