producer : Chan Int -> Chan () -> Chan () -> Task ()
producer values ready done = do
    send values 20
    send ready ()
    send values 22
    send done ()

reportPending : Maybe () -> Task ()
reportPending Nothing = printLine "producer-blocked"
reportPending (Just _) = printLine "unexpected-completion"

main : Task ()
main = do
    values <- newBoundedChan 1
    ready <- newChan
    done <- newChan
    _ <- spawn (producer values ready done)
    _ <- recv ready
    pending <- tryRecv done
    reportPending pending
    first <- recv values
    second <- recv values
    _ <- recv done
    printLine (show (first + second))
