import std.net(*)
import std.task(*)
import std.time(*)
import std.channel(*)

slow : Task String
slow =
    finally
        (bindIO (sleep (milliseconds 30)) (fun elapsed => pureIO "slow"))
        (printLine "slow-cleaned")

fast : Task String
fast = do
    sleep (milliseconds 10)
    pureIO "fast"

errorLoser : Task String
errorLoser =
    finally
        (bindIO (sleep (milliseconds 100)) (fun elapsed => pureIO "late"))
        (printLine "error-loser-cleaned")

failClosed : Task String
failClosed = failIO (IOError ResourceVanished "channel operation" "channel is closed")

reportWinner : Either String String -> Task ()
reportWinner winner =
    case winner of
        Left value => printLine (appendString "left=" value)
        | Right value => printLine (appendString "right=" value)

reportError : IOError -> Task ()
reportError (IOError ResourceVanished _ _) = printLine "race-error=ChannelClosed"
reportError other = printLine ("unexpected I/O error: " <> show other)

main : Task ()
main = do
    winner <- race slow fast
    reportWinner winner
    catchError
        (bindIO
            (race failClosed errorLoser)
            (fun unexpected => printLine "unexpected race success"))
        reportError
