import std.thread(*)

main : Task ()
main = do
    _ <- spawn (printLine "child")
    printLine "main"
