main : Task ()
main = do
    value <- pureIO "pure-value"
    printLine value
