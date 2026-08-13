import std.list
import std.random
import std.task
import std.text(intToString)

printInts : Int -> StdGen -> Task ()
printInts remaining generator =
    if remaining <= 0 then
        pureIO ()
    else
        case randomInt generator of
            MkPair value next => do
                printLine (intToString value)
                printInts (remaining - 1) next

printRanges : Int -> StdGen -> Task ()
printRanges remaining generator =
    if remaining <= 0 then
        pureIO ()
    else
        case randomRange (-10) 10 generator of
            MkPair value next => do
                printLine (intToString value)
                printRanges (remaining - 1) next

printFresh : StdGen -> Task ()
printFresh generator =
    case randomInt generator of
        MkPair value next => printLine (intToString value)

invalidRange : Task ()
invalidRange =
    bindIO
        (pureIO ())
        (fun ready =>
            case randomRange 5 4 (mkStdGen 0) of
                MkPair value next => printLine "unexpected valid range")

reportDefect : Defect -> Task ()
reportDefect defect =
    case defect of
        Error message => printLine (appendString "invalid bounds: " message)
        | DivisionByZero => printLine "unexpected DivisionByZero"
        | StackExhausted => printLine "unexpected StackExhausted"
        | HeapExhausted => printLine "unexpected HeapExhausted"

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
    catchDefect invalidRange reportDefect
