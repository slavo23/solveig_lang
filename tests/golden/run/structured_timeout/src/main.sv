import std.task
import std.time
slow : Task String
slow =
    finally
        (bindIO (sleep 30) (fun elapsed => pureIO "slow"))
        (printLine "timeout-loser-cleaned")

fast : Task String
fast = do
    sleep 5
    pureIO "fast"

report : Maybe String -> Task ()
report result =
    case result of
        Nothing => printLine "timed-out"
        | Just value => printLine (appendString "completed=" value)

main : Task ()
main = do
    first <- timeout 10 slow
    report first
    second <- timeout 20 fast
    report second
