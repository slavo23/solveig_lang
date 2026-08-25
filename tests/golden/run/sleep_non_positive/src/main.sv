import std.time(*)

main : Task ()
main = do
    sleep (milliseconds 0)
    sleep (milliseconds (0 - 10))
    printLine "immediate"
