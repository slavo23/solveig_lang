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
        | InvalidUtf8 message => printLine "unexpected UTF-8 error"
        | ResolveHostError message => printLine "unexpected resolve host error"
        | ConnectError message => printLine "unexpected connect error"
        | ListenError message => printLine "unexpected listen error"
        | AcceptError message => printLine "unexpected accept error"
        | SendError message => printLine "unexpected send error"
        | RecvError message => printLine "unexpected recv error"
        | CloseSocketError message => printLine "unexpected close socket error"
        | OpenHandleError path message => printLine "unexpected open handle error"
        | CloseHandleError message => printLine "unexpected close handle error"
        | FlushHandleError message => printLine "unexpected flush handle error"
        | ReadHandleError message => printLine "unexpected read handle error"
        | WriteHandleError message => printLine "unexpected write handle error"
        | SetBufferingError message => printLine "unexpected buffering error"

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
