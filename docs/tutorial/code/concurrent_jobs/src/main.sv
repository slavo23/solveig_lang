import std.task(concurrentHostError, mapConcurrentlyBoundedOrdered)
import std.time(milliseconds, sleep)

process : Int -> Task Int
process value = do
    sleep (milliseconds ((5 - value) * 10))
    pureIO (value * value)

main : Task ()
main = do
    results <- mapError concurrentHostError (mapConcurrentlyBoundedOrdered 3 process [1, 2, 3, 4])
    printLine ("results: " <> show results)
