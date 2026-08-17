import std.bytes(bytesAppend, bytesIndex, bytesLength, bytesSlice, stringToBytes)
import std.channel(newBoundedChan, recv, send)
import std.env(getArgs)
import std.io(readFileBytes)
import std.text(charsToString, stringToChars)
import std.thread(spawn)

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

applyMany : (Int -> Int) -> Int -> Int -> Int
applyMany transform remaining value =
    if remaining == 0 then value
    else applyMany transform (remaining - 1) (transform value)

addBy : Int -> Int -> Int
addBy amount = fun value => value + amount

closureMain : Int
closureMain = applyMany (addBy 7) 200000 1

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
