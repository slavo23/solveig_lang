import std.task(*)
import std.time(*)
import std.channel(*)

data WorkError = WorkFailed

data PooledError = PooledFailed

pause : Int -> IO WorkError ()
pause milliseconds =
    catchError
        (sleep (std.time.milliseconds milliseconds))
        (fun timerError => error "structured test timer failed")

logLine : String -> IO WorkError ()
logLine text =
    catchError (printLine text) (fun outputError => error "structured test output failed")

slow : IO WorkError Int
slow = finally (bindIO (pause 30) (fun elapsed => pureIO 99)) (logLine "slow-cleaned")

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
    sleep (std.time.milliseconds (pooledDelay value))
    pureIO (value * 10)

pooledDelay : Int -> Int
pooledDelay 1 = 40
pooledDelay _ = 10

pooledFailure : Int -> IO PooledError Int
pooledFailure 2 = failIO PooledFailed
pooledFailure value = pureIO (value * 10)

boundedWork : Chan Int -> Chan () -> Int -> Task Int
boundedWork started release value = do
    send started value
    _ <- recv release
    pureIO value

noImmediateValue : Maybe a -> Bool
noImmediateValue Nothing = True
noImmediateValue (Just _) = False

runBoundedPool : Chan Int -> Chan () -> Chan [Int] -> Task ()
runBoundedPool started release resultsChannel = do
    results <-
        mapError
            concurrentHostError
            (mapConcurrentlyBoundedOrdered 2 (boundedWork started release) [1, 2, 3])
    send resultsChannel results

checkBoundedPool : Chan Int -> Chan () -> Chan [Int] -> ThreadId -> Task ()
checkBoundedPool started release resultsChannel _ = do
    _ <- recv started
    _ <- recv started
    thirdBeforeRelease <- tryRecv started
    send release ()
    _ <- recv started
    send release ()
    send release ()
    results <- recv resultsChannel
    printLine ("bounded=" <> show (noImmediateValue thirdBeforeRelease) <> ":" <> show results)

completionWork : Chan Int -> (Chan (), Chan (), Chan (), Chan ()) -> Int -> Task Int
completionWork started gates value = do
    send started value
    case (gates, value) of
        ((first, _, _, _), 1) => recv first
        | ((_, second, _, _), 2) => recv second
        | ((_, _, third, _), 3) => recv third
        | ((_, _, _, fourth), _) => recv fourth
    pureIO (value * 10)

runCompletionPool : Chan Int -> (Chan (), Chan (), Chan (), Chan ()) -> Chan [Int] -> Task ()
runCompletionPool started gates resultsChannel = do
    results <-
        mapError
            concurrentHostError
            (mapConcurrentlyBoundedCompletionOrder 2 (completionWork started gates) [1, 2, 3, 4])
    send resultsChannel results

checkCompletionPool : Chan Int -> (Chan (), Chan (), Chan (), Chan ()) -> Chan [Int] -> ThreadId -> Task ()
checkCompletionPool started gates resultsChannel _ = do
    _ <- recv started
    _ <- recv started
    case gates of
        (first, second, third, fourth) => do
            send second ()
            _ <- recv started
            send third ()
            _ <- recv started
            send fourth ()
            send first ()
    results <- recv resultsChannel
    printLine ("completion=" <> show results)

reportPooledFailure : ConcurrentError PooledError -> Task ()
reportPooledFailure (ConcurrentWorker PooledFailed) = printLine "pooled-error=PooledFailed"
reportPooledFailure _ = error "unexpected pooled error"

expectPooledFailure : Task ()
expectPooledFailure =
    catchError
        (bindIO
            (mapConcurrentlyBoundedOrdered 2 pooledFailure [1, 2, 3, 4])
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
            (mapConcurrentlyBoundedOrdered 0 pooledFailure [1])
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
        (leftValue, rightValue) => printLine (appendString "pair=7," rightValue)
    ordered <-
        mapError concurrentHostError (mapConcurrentlyBoundedOrdered 2 pooledWork [1, 2, 3, 4])
    printLine ("ordered=" <> show ordered)
    completionStarted <- newChan
    firstGate <- newChan
    secondGate <- newChan
    thirdGate <- newChan
    fourthGate <- newChan
    completionResults <- newChan
    let completionGates = (firstGate, secondGate, thirdGate, fourthGate)
    withThread
        (runCompletionPool completionStarted completionGates completionResults)
        (checkCompletionPool completionStarted completionGates completionResults)
    started <- newChan
    release <- newChan
    boundedResults <- newChan
    withThread
        (runBoundedPool started release boundedResults)
        (checkBoundedPool started release boundedResults)
    expectPooledFailure
    expectInvalidPool
    measured <- std.time.measure (pureIO 42)
    case measured of
        (elapsed, value) =>
            printLine ("measured=" <> show (elapsed >= nanoseconds 0) <> ":" <> show value)
