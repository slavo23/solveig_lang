data WorkError = WorkFailed

pause : Int -> IO WorkError ()
pause milliseconds =
    catchError
        (sleep milliseconds)
        (fun timerError => error "structured test timer failed")

logLine : String -> IO WorkError ()
logLine text =
    catchError
        (printLine text)
        (fun outputError => error "structured test output failed")

slow : IO WorkError Int
slow =
    finally
        (bindIO (pause 30) (fun elapsed => pureIO 99))
        (logLine "slow-cleaned")

failFast : IO WorkError String
failFast = do
    pause 10
    failIO WorkFailed

leftSuccess : IO WorkError Int
leftSuccess = do
    pause 20
    pureIO 7

rightSuccess : IO WorkError String
rightSuccess = do
    pause 5
    pureIO "ok"

reportFailure : WorkError -> Task ()
reportFailure WorkFailed = printLine "failed=WorkFailed"

main : Task ()
main = do
    catchError
        (bindIO
            (concurrently slow failFast)
            (fun unexpected => error "unexpected concurrent success"))
        reportFailure
    pair <-
        catchError
            (concurrently leftSuccess rightSuccess)
            (fun unexpected => error "unexpected concurrent failure")
    case pair of
        MkPair leftValue rightValue =>
            printLine (appendString "pair=7," rightValue)
