import std.channel(*)

closedRecv : IOError -> Task Int
closedRecv (IOError ResourceVanished _ _) = do
    printLine "recv-closed"
    pureIO 0
closedRecv _ = pureIO (-1)

closedSend : IOError -> Task ()
closedSend (IOError ResourceVanished _ _) = printLine "send-closed"
closedSend _ = printLine "unexpected-error"

closedTry : IOError -> Task (Maybe Int)
closedTry (IOError ResourceVanished _ _) = do
    printLine "try-closed"
    pureIO Nothing
closedTry _ = pureIO (Just (-1))

main : Task ()
main = do
    channel <- newChan
    send channel 7
    closeChan channel
    queued <- recv channel
    printLine (show queued)
    afterClose <- catchError (recv channel) closedRecv
    printLine (show afterClose)
    catchError (send channel 8) closedSend
    _ <- catchError (tryRecv channel) closedTry
    pureIO ()
