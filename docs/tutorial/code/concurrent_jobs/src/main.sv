import std.task(mapConcurrentlyBounded)
import std.time(sleep)

process : Int -> Task Int
process value = do
    sleep ((5 - value) * 10)
    pureIO (value * value)

main : Task ()
main = do
    results <- mapConcurrentlyBounded 3 process [1, 2, 3, 4]
    printLine ("results: " <> show results)
