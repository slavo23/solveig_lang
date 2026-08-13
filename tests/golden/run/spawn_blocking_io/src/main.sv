import std.thread
reader : Task ()
reader = do
    line <- readLine
    printLine (appendString "worker read: " line)

waitForWorkers : Int -> Task ()
waitForWorkers n =
    if n == 0 then pureIO ()
    else do
        _ <- readLine
        waitForWorkers (n - 1)

main : Task ()
main = do
    _ <- spawn reader
    printLine "main stayed runnable"
    waitForWorkers 8
