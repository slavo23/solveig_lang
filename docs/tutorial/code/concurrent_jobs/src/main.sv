import std.channel(*)
import std.task(concurrentHostError, concurrently, mapConcurrentlyBoundedOrdered)
import std.time(milliseconds, sleep)

process : Int -> Task Int
process value = do
    sleep $ milliseconds $ (5 - value) * 10
    pureIO $ value * value

awaitStarts : Int -> Chan Int -> Task ()
awaitStarts 0 _ = pureIO ()
awaitStarts remaining started = do
    _ <- recv started
    awaitStarts (remaining - 1) started

releaseWorkers : Int -> Chan () -> Task ()
releaseWorkers 0 _ = pureIO ()
releaseWorkers remaining release = do
    send release ()
    releaseWorkers (remaining - 1) release

waitForRelease : Chan Int -> Chan () -> Int -> Task String
waitForRelease started release job = do
    send started job
    _ <- recv release
    pureIO $ "invoice-" <> show job

coordinate : Chan Int -> Chan () -> Task ()
coordinate started release = do
    awaitStarts 3 started
    releaseWorkers 3 release

proveOverlap : Task [String]
proveOverlap = do
    started <- newChan
    release <- newChan
    (results, _) <-
        concurrently
            (mapError concurrentHostError $
                mapConcurrentlyBoundedOrdered 3 (waitForRelease started release) [101, 102, 103])
            (coordinate started release)
    pureIO results

main : Task ()
main = do
    results <- mapError concurrentHostError (mapConcurrentlyBoundedOrdered 3 process [1, 2, 3, 4])
    printLine ("results: " <> show results)
    overlapping <- proveOverlap
    printLine $ "overlap: " <> show overlapping
