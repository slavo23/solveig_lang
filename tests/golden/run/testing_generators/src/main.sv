import std.channel(*)
import std.random(mkStdGen)
import std.testing(*)

integerGenerator : Gen Int
integerGenerator =
    case chooseInt (-10) 10 of
        Nothing => error "valid integer generator bounds were rejected"
        | Just generator => generator

sampleGenerator : Gen [Int]
sampleGenerator =
    case listOf 5 (mapGen (fun value => value * 2) integerGenerator) of
        Nothing => error "valid generator length was rejected"
        | Just generator => generator

zeroGenerator : Gen Int
zeroGenerator =
    case chooseInt 0 0 of
        Nothing => error "valid singleton generator was rejected"
        | Just generator => generator

reportProperty : Either TestFailure StdGen -> Task ()
reportProperty (Right _) = printLine "property=pass"
reportProperty (Left failure) = printLine ("property=" <> failureMessage failure)

reportAssertion : Either TestFailure () -> String
reportAssertion (Right _) = "pass"
reportAssertion (Left failure) = failureMessage failure

wasReleased : Maybe () -> Bool
wasReleased Nothing = False
wasReleased (Just _) = True

invalidGeneratorsRejected : Bool
invalidGeneratorsRejected =
    case (chooseInt 1 0, listOf (-1) (constantGen ())) of
        (Nothing, Nothing) => True
        | _ => False

main : Task ()
main = do
    let seed = mkStdGen 12345
    case (generate sampleGenerator seed, generate sampleGenerator seed) of
        ((first, _), (second, _)) =>
            do
                printLine ("samples=" <> show first)
                printLine ("reproducible=" <> show (first == second))
    case generate (pairGen integerGenerator (constantGen "fixed")) seed of
        (pair, _) => printLine ("pair=" <> show pair)
    printLine ("invalid-generators=" <> show invalidGeneratorsRejected)
    reportProperty
        (checkProperty
            "bounded integers"
            50
            integerGenerator
            (fun value => and (value >= -10) (value <= 10))
            seed)
    reportProperty (checkProperty "zero is positive" 3 zeroGenerator (fun value => value > 0) seed)
    reportProperty (checkProperty "invalid count" 0 (constantGen ()) (fun value => True) seed)
    released <- newChan
    resourceCheck <-
        assertWithResource
            "resource value"
            (pureIO 42)
            (fun value => send released ())
            (fun value => pureIO (value == 42))
    cleanup <- tryRecv released
    printLine
        ("resource=" <> reportAssertion resourceCheck <> ",released=" <> show (wasReleased cleanup))
