import std.list(*)
import std.random(*)
import std.text(intToString)

printInts : Int -> StdGen -> Task ()
printInts remaining generator =
    if remaining <= 0
        then pureIO ()
        else
            case randomInt generator of
                (value, next) => do
                    printLine (intToString value)
                    printInts (remaining - 1) next

printRanges : Int -> StdGen -> Task ()
printRanges remaining generator =
    if remaining <= 0
        then pureIO ()
        else
            case randomRange (-10) 10 generator of
                Nothing => printLine "unexpected invalid range"
                | Just (value, next) => do
                    printLine (intToString value)
                    printRanges (remaining - 1) next

printFresh : StdGen -> Task ()
printFresh generator =
    case randomInt generator of
        (value, next) => printLine (intToString value)

printInvalidRange : Task ()
printInvalidRange =
    case randomRange 5 4 (mkStdGen 0) of
        Nothing => printLine "invalid bounds"
        | Just _ => printLine "unexpected valid range"

main : Task ()
main = do
    printLine "fixed integers"
    printInts 5 (mkStdGen 0)
    printLine "inclusive range"
    printRanges 5 (mkStdGen 0)
    printLine "fresh generators"
    first <- newStdGen
    second <- newStdGen
    printFresh first
    printFresh second
    printInvalidRange
