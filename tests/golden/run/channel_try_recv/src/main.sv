import std.channel
report : Maybe Int -> Task ()
report Nothing = printLine "empty"
report (Just value) = printLine (show value)

main : Task ()
main = do
    channel <- newChan
    empty <- tryRecv channel
    report empty
    send channel 42
    present <- tryRecv channel
    report present
