import std.env
main : Task ()
main = do
    printStr "name> "
    name <- readLine
    args <- getArgs
    printLine (appendString "hello, " name)
    mapIO_ printLine args
