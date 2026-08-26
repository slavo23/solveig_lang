import std.map(*)
import std.hashmap
import std.hashset
import std.list(take, drop, range, rangeInclusive, length, isEmpty)
import std.queue(*)
import std.set(*)
import std.text(lines, splitOnce)
import std.vector(*)

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
        [ (3, "three")
        , (1, "one")
        , (4, "four")
        , (2, "old")
        , (2, "two")
        ]

queueExample : std.queue.Queue Int
queueExample =
    std.queue.enqueue
        (std.queue.enqueue
            (std.queue.enqueue std.queue.empty 1)
            2)
        3

minimumInt : Int
minimumInt = (0 - 9223372036854775807) - 1

printDequeued : std.queue.Queue Int -> Task ()
printDequeued queue =
    case std.queue.dequeue queue of
        Nothing => printLine "empty"
        | Just (value, rest) => do
            printLine (show value)
            printLine (show (std.queue.toList rest))

main : Task ()
main = do
    printLine (show (std.vector.length vectorExample))
    printLine (show (std.vector.index vectorExample 3))
    printLine (show (std.vector.index vectorExample 8))
    printLine (show (std.vector.toList updatedVector))
    printLine (show (std.vector.toList ((+) 1 <$> vectorExample)))
    printLine (show vectorExample)
    printLine (show (hash vectorExample == hash (std.vector.fromList [10, 20, 30, 40, 50])))
    printLine (show (traverse Just vectorExample))
    printLine (show (std.map.size mapExample))
    printLine (show (std.map.keys mapExample))
    printLine (show (std.map.lookup 2 mapExample))
    printLine (show (std.map.keys (std.map.delete 3 mapExample)))
    printLine (show (std.map.values (fmap (fun text => text <> "!") mapExample)))
    printLine (show mapExample)
    printLine (show (hash mapExample == hash (std.map.fromList (std.map.toAscList mapExample))))
    printLine (show (traverse Just mapExample))
    let hashed = std.hashmap.fromList [(3, "three"), (1, "one"), (3, "updated")]
    printLine (show (std.hashmap.toAscList hashed))
    printLine (show (std.hashset.toAscList (std.hashset.fromList [3, 1, 3, 2])))
    let first = std.set.fromList [5, 1, 3, 3]
    let second = std.set.fromList [2, 3, 4]
    printLine (show (std.set.toAscList (std.set.union first second)))
    printLine (show (std.set.toAscList (std.set.intersection first second)))
    printLine (show (std.set.toAscList (std.set.difference first second)))
    printLine (show first)
    printLine (show (std.set.isEmpty (std.set.fromList [1])))
    printLine (show (hash first == hash (std.set.fromList [3, 5, 1])))
    printLine (show (foldl (+) 0 first))
    printLine (show (std.queue.length queueExample))
    printLine (show queueExample)
    printLine (show (hash queueExample == hash (std.queue.fromList [1, 2, 3])))
    printLine (show (traverse Just queueExample))
    printLine (show (fmap ((+) 1) queueExample))
    printDequeued queueExample
    printLine (show (std.list.take (0 - 1) [1, 2, 3]))
    printLine (show (std.list.drop (0 - 1) [1, 2, 3]))
    printLine (show (std.list.range 2 5))
    printLine (show (std.list.rangeInclusive 2 5))
    printLine (show (std.list.range 5 2))
    printLine (show (std.list.range minimumInt (minimumInt + 1)))
    printLine (show (std.list.rangeInclusive minimumInt minimumInt))
    printLine (show (std.list.length [10, 20, 30]))
    printLine (show (std.list.isEmpty (std.list.take 0 [1])))
    printLine (show (std.list.isEmpty [1]))
    printLine (show (std.text.lines "alpha\n\nbeta\n"))
    printLine (show (std.text.splitOnce ':' "key:value:rest"))
    printLine (show (std.text.splitOnce ':' "missing"))
