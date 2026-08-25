import std.thread(*)
import std.channel(*)

spawnProducers : Chan Int -> Int -> Task ()
spawnProducers channel remaining =
    if remaining == 0 then pureIO ()
    else do
        _ <- spawn (send channel remaining)
        spawnProducers channel (remaining - 1)

sumMessages : Chan Int -> Int -> Int -> Task Int
sumMessages channel remaining total =
    if remaining == 0 then pureIO total
    else do
        value <- recv channel
        sumMessages channel (remaining - 1) (total + value)

main : Task ()
main = do
    channel <- newChan
    spawnProducers channel 100
    total <- sumMessages channel 100 0
    printLine (show total)
