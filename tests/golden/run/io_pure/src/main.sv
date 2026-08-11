main : IO () ()
main =
    do
        value <- pureIO "pure-value"
        printLine value
