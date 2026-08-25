import std.thread(*)
import std.channel(*)

worker : Chan String -> Chan String -> Task ()
worker requests replies = do
    request <- recv requests
    send replies (appendString request "-pong")

main : Task ()
main = do
    requests <- newChan
    replies <- newChan
    _ <- spawn (worker requests replies)
    send requests "ping"
    reply <- recv replies
    printLine reply
