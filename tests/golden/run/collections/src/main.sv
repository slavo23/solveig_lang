import std.map
import std.list
import std.queue
import std.set
import std.vector

vectorExample : std.vector.Vector Int
vectorExample = std.vector.fromList [10, 20, 30, 40, 50]

updatedVector : std.vector.Vector Int
updatedVector =
    case std.vector.update vectorExample 2 99 of
        Nothing => vectorExample
        | Just vector => vector

mapExample : std.map.Map Int String
mapExample =
    std.map.fromList
        [ std.map.Entry 3 "three"
        , std.map.Entry 1 "one"
        , std.map.Entry 4 "four"
        , std.map.Entry 2 "old"
        , std.map.Entry 2 "two"
        ]

queueExample : std.queue.Queue Int
queueExample =
    std.queue.enqueue
        (std.queue.enqueue
            (std.queue.enqueue std.queue.empty 1)
            2)
        3

printDequeued : std.queue.Queue Int -> Task ()
printDequeued queue =
    case std.queue.dequeue queue of
        Nothing => printLine "empty"
        | Just (MkPair value rest) => do
            printLine (show value)
            printLine (show (std.queue.toList rest))

main : Task ()
main = do
    printLine (show (std.vector.length vectorExample))
    printLine (show (std.vector.index vectorExample 3))
    printLine (show (std.vector.index vectorExample 8))
    printLine (show (std.vector.toList updatedVector))
    printLine (show (std.vector.toList ((+) 1 <$> vectorExample)))
    printLine (show (std.map.size mapExample))
    printLine (show (std.map.keys mapExample))
    printLine (show (std.map.lookup 2 mapExample))
    printLine (show (std.map.keys (std.map.delete 3 mapExample)))
    printLine (show (std.map.values (fmap (fun text => text <> "!") mapExample)))
    let first = std.set.fromList [5, 1, 3, 3]
    let second = std.set.fromList [2, 3, 4]
    printLine (show (std.set.toAscList (std.set.union first second)))
    printLine (show (std.set.toAscList (std.set.intersection first second)))
    printLine (show (std.set.toAscList (std.set.difference first second)))
    printLine (show (std.queue.length queueExample))
    printDequeued queueExample
    printLine (show (std.list.take (0 - 1) [1, 2, 3]))
    printLine (show (std.list.drop (0 - 1) [1, 2, 3]))
