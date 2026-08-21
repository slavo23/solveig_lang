import std.bytes(bytesAppend, bytesIndex, bytesLength, bytesSlice, bytesSpanRange, stringToBytes)
import std.channel(newBoundedChan, recv, send)
import std.env(getArgs)
import std.io(readFileBytes)
import std.text(charsToString, stringToChars)
import std.thread(spawn)
import std.map
import std.queue
import std.vector

compilerPipelineMain : Int
compilerPipelineMain = 0

jsonDigitBytes : Bytes
jsonDigitBytes = stringToBytes
    "0123456789012345678901234567890123456789012345678901234567890123012345678901234567890123456789012345678901234567890123456789012301234567890123456789012345678901234567890123456789012345678901230123456789012345678901234567890123456789012345678901234567890123"

jsonBulkScanLoop : Int -> Int -> Int
jsonBulkScanLoop remaining total =
    if remaining == 0 then total
    else jsonBulkScanLoop (remaining - 1)
        (total + bytesSpanRange jsonDigitBytes 0 48 57)

jsonScanBulkMain : Int
jsonScanBulkMain = jsonBulkScanLoop 500 0

jsonScalarSpan : Bytes -> Int -> Int
jsonScalarSpan bytes offset =
    case bytesIndex bytes offset of
        Just byte =>
            if and (byte >= 48) (byte <= 57) then jsonScalarSpan bytes (offset + 1)
            else offset
        | Nothing => offset

jsonScalarScanLoop : Int -> Int -> Int
jsonScalarScanLoop remaining total =
    if remaining == 0 then total
    else jsonScalarScanLoop (remaining - 1) (total + jsonScalarSpan jsonDigitBytes 0)

jsonScanScalarMain : Int
jsonScanScalarMain = jsonScalarScanLoop 500 0

buildList : Int -> [Int] -> [Int]
buildList remaining values =
    if remaining == 0 then values
    else buildList (remaining - 1) (remaining :: values)

sumList : [Int] -> Int -> Int
sumList values total =
    case values of
        [] => total
        | value :: rest => sumList rest (total + value)

allocationMain : Int
allocationMain = sumList (buildList 50000 []) 0

arithmeticLoop : Int -> Int -> Int
arithmeticLoop remaining value =
    if remaining == 0 then value
    else
        arithmeticLoop
            (remaining - 1)
            (mod ((value * 1664525) + remaining) 2147483647)

arithmeticMain : Int
arithmeticMain = arithmeticLoop 200000 1013904223

addGeneric : forall a. Num a => a -> a -> a
addGeneric left right = left + right

constraintGenericLoop : Int -> Int -> Int
constraintGenericLoop remaining total =
    if remaining == 0 then total
    else constraintGenericLoop (remaining - 1) (addGeneric total remaining)

constraintGenericMain : Int
constraintGenericMain = constraintGenericLoop 200000 0

addConcrete : Int -> Int -> Int
addConcrete left right = left + right

constraintConcreteLoop : Int -> Int -> Int
constraintConcreteLoop remaining total =
    if remaining == 0 then total
    else constraintConcreteLoop (remaining - 1) (addConcrete total remaining)

constraintConcreteMain : Int
constraintConcreteMain = constraintConcreteLoop 200000 0

applyMany : (Int -> Int) -> Int -> Int -> Int
applyMany transform remaining value =
    if remaining == 0 then value
    else applyMany transform (remaining - 1) (transform value)

addBy : Int -> Int -> Int
addBy amount = fun value => value + amount

closureMain : Int
closureMain = applyMany (addBy 7) 200000 1

applyStaticMany : Int -> Int -> Int
applyStaticMany remaining value =
    if remaining == 0 then value
    else applyStaticMany (remaining - 1) ((fun item => item + 1) value)

staticClosureMain : Int
staticClosureMain = applyStaticMany 200000 1

data Tree = Leaf Int | Branch Tree Tree

sharedTree : Int -> Tree
sharedTree depth =
    if depth == 0 then Leaf 1
    else
        let child = sharedTree (depth - 1)
        in Branch child child

sumTree : Tree -> Int
sumTree tree =
    case tree of
        Leaf value => value
        | Branch left right => sumTree left + sumTree right

repeatTreeSum : Tree -> Int -> Int -> Int
repeatTreeSum tree remaining total =
    if remaining == 0 then total
    else repeatTreeSum tree (remaining - 1) (total + sumTree tree)

patternMain : Int
patternMain = repeatTreeSum (sharedTree 16) 8 0

byteLoop : Int -> Bytes -> Int -> Int
byteLoop remaining bytes total =
    if remaining == 0 then total
    else
        let half = bytesSlice bytes 0 8
        in let joined = bytesAppend half half
        in case bytesIndex joined 0 of
            Nothing => byteLoop (remaining - 1) joined total
            | Just first =>
                byteLoop
                    (remaining - 1)
                    joined
                    (total + bytesLength joined + first)

stringBytesMain : Int
stringBytesMain =
    let text = charsToString (stringToChars "abcdefghijklmnop")
    in byteLoop 20000 (stringToBytes text) 0

readMany : String -> Int -> Int -> Task Int
readMany path remaining completed =
    if remaining == 0 then pureIO completed
    else do
        bytes <- readFileBytes path
        let next = if bytesLength bytes == 0 then completed else completed + 1
        readMany path (remaining - 1) next

fileIoMain : Task ()
fileIoMain = do
    args <- getArgs
    case args of
        path :: _ => do
            completed <- readMany path 64 0
            printLine (show completed)
        | [] => error "file IO benchmark requires an input path"

sendMany : Chan Int -> Int -> Task ()
sendMany channel remaining =
    if remaining == 0 then pureIO ()
    else do
        send channel remaining
        sendMany channel (remaining - 1)

receiveMany : Chan Int -> Int -> Int -> Task Int
receiveMany channel remaining total =
    if remaining == 0 then pureIO total
    else do
        value <- recv channel
        receiveMany channel (remaining - 1) (total + value)

channelMain : Task ()
channelMain = do
    channel <- newBoundedChan 64
    _ <- spawn (sendMany channel 10000)
    total <- receiveMany channel 10000 0
    printLine (show total)

benchmarkValues : [Int]
benchmarkValues = buildList 1024 []

benchmarkVector : std.vector.Vector Int
benchmarkVector = std.vector.fromList benchmarkValues

listIndex : forall a. [a] -> Int -> Maybe a
listIndex [] offset = Nothing
listIndex (value :: rest) 0 = Just value
listIndex (value :: rest) offset = listIndex rest (offset - 1)

sumIndexed : (Int -> Maybe Int) -> Int -> Int -> Int
sumIndexed lookupIndex remaining total =
    if remaining == 0 then total
    else
        case lookupIndex (mod (remaining * 37) 1024) of
            Nothing => error "collection benchmark index escaped its bounds"
            | Just value => sumIndexed lookupIndex (remaining - 1) (total + value)

vectorIndexMain : Int
vectorIndexMain = sumIndexed (std.vector.index benchmarkVector) 2000 0

listIndexMain : Int
listIndexMain = sumIndexed (listIndex benchmarkValues) 2000 0

mapEntries : [Int] -> [std.map.Entry Int Int]
mapEntries [] = []
mapEntries (value :: rest) = std.map.Entry value (value * 2) :: mapEntries rest

benchmarkMap : std.map.Map Int Int
benchmarkMap = std.map.fromList (mapEntries benchmarkValues)

associationLookup : Int -> [(Int, Int)] -> Maybe Int
associationLookup key [] = Nothing
associationLookup key ((entryKey, value) :: rest) =
    if key == entryKey then Just value else associationLookup key rest

associationEntries : [Int] -> [(Int, Int)]
associationEntries [] = []
associationEntries (value :: rest) = (value, value * 2) :: associationEntries rest

benchmarkAssociations : [(Int, Int)]
benchmarkAssociations = associationEntries benchmarkValues

mapLookupMain : Int
mapLookupMain = sumIndexed (fun key => std.map.lookup (key + 1) benchmarkMap) 2000 0

associationLookupMain : Int
associationLookupMain =
    sumIndexed (fun key => associationLookup (key + 1) benchmarkAssociations) 2000 0

fillQueue : Int -> std.queue.Queue Int -> std.queue.Queue Int
fillQueue remaining queue =
    if remaining == 0 then queue
    else fillQueue (remaining - 1) (std.queue.enqueue queue remaining)

drainQueue : std.queue.Queue Int -> Int -> Int
drainQueue queue total =
    case std.queue.dequeue queue of
        Nothing => total
        | Just (value, rest) => drainQueue rest (total + value)

queueMain : Int
queueMain = drainQueue (fillQueue 1000 std.queue.empty) 0

fillNaiveQueue : Int -> [Int] -> [Int]
fillNaiveQueue remaining queue =
    if remaining == 0 then queue
    else fillNaiveQueue (remaining - 1) (queue ++ [remaining])

drainNaiveQueue : [Int] -> Int -> Int
drainNaiveQueue [] total = total
drainNaiveQueue (value :: rest) total = drainNaiveQueue rest (total + value)

naiveQueueMain : Int
naiveQueueMain = drainNaiveQueue (fillNaiveQueue 1000 []) 0
