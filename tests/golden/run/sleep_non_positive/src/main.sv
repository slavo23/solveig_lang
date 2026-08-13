import std.time
main : Task ()
main = do
    sleep 0
    sleep (0 - 10)
    printLine "immediate"
