import std.env
main : Task ()
main = do
    args <- getArgs
    mapIO_ printLine args
