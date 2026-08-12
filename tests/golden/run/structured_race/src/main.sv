slow : Task String
slow =
    finally
        (bindIO (sleep 30) (fun elapsed => pureIO "slow"))
        (printLine "slow-cleaned")

fast : Task String
fast = do
    sleep 10
    pureIO "fast"

errorLoser : Task String
errorLoser =
    finally
        (bindIO (sleep 100) (fun elapsed => pureIO "late"))
        (printLine "error-loser-cleaned")

failClosed : Task String
failClosed = failIO ChannelClosed

reportWinner : Either String String -> Task ()
reportWinner winner =
    case winner of
        Left value => printLine (appendString "left=" value)
        | Right value => printLine (appendString "right=" value)

reportError : IOError -> Task ()
reportError ioError =
    case ioError of
        ChannelClosed => printLine "race-error=ChannelClosed"
        | StdoutError message => printLine "unexpected stdout error"
        | StdinError message => printLine "unexpected stdin error"
        | ReadFileError path message => printLine "unexpected read error"
        | WriteFileError path message => printLine "unexpected write error"
        | AppendFileError path message => printLine "unexpected append error"

main : Task ()
main = do
    winner <- race slow fast
    reportWinner winner
    catchError
        (bindIO
            (race
                failClosed
                errorLoser)
            (fun unexpected => printLine "unexpected race success"))
        reportError
