import std.task(*)
import std.time(*)

data WorkError = WorkFailed

data PooledError = PooledFailed

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

pooledWork : Int -> Task Int
pooledWork value = do
    sleep ((5 - value) * 5)
    pureIO (value * 10)

pooledFailure : Int -> IO PooledError Int
pooledFailure 2 = failIO PooledFailed
pooledFailure value = pureIO (value * 10)

reportPooledFailure : ConcurrentError PooledError -> Task ()
reportPooledFailure (ConcurrentWorker PooledFailed) = printLine "pooled-error=PooledFailed"
reportPooledFailure _ = error "unexpected pooled error"

expectPooledFailure : Task ()
expectPooledFailure =
    catchError
        (bindIO
            (mapConcurrentlyBounded 2 pooledFailure [1, 2, 3, 4])
            (fun unexpected => error ("unexpected pooled success: " <> show unexpected)))
        reportPooledFailure

reportInvalidPool : ConcurrentError PooledError -> Task ()
reportInvalidPool (ConcurrentInfrastructure (HostError InvalidArgument _ _)) =
    printLine "pooled-error=InvalidArgument"
reportInvalidPool _ = error "unexpected invalid pool error"

expectInvalidPool : Task ()
expectInvalidPool =
    catchError
        (bindIO
            (mapConcurrentlyBounded 0 pooledFailure [1])
            (fun unexpected => error "unexpected invalid pool success"))
        reportInvalidPool

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
        (leftValue, rightValue) =>
            printLine (appendString "pair=7," rightValue)
    pooled <- mapError concurrentHostError (mapConcurrentlyBounded 2 pooledWork [1, 2, 3, 4])
    printLine ("pooled=" <> show pooled)
    expectPooledFailure
    expectInvalidPool
    measured <- std.time.measure (pureIO 42)
    case measured of
        (elapsed, value) =>
            printLine ("measured=" <> show (elapsed >= 0) <> ":" <> show value)
