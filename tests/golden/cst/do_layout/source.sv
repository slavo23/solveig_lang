main : Task ()
main = do
    printLine "first"
    name <- readLine
    let greeting = appendString "hello, " name
    printLine greeting
