import std.process(exitFailure)
import std.random(mkStdGen)
import std.testing(*)

lineTotal : Int -> Int -> Int
lineTotal quantity unitPriceCents = quantity * unitPriceCents

boundedInt : Int -> Int -> Gen Int
boundedInt lower upper =
    case chooseInt lower upper of
        Nothing => error "tutorial generator bounds are invalid"
        | Just generator => generator

nonNegativeLine : Gen (Int, Int)
nonNegativeLine = pairGen (boundedInt 0 100) (boundedInt 0 100000)

lineTotalIsNonNegative : (Int, Int) -> Bool
lineTotalIsNonNegative (quantity, unitPriceCents) = lineTotal quantity unitPriceCents >= 0

discardGenerator : Either TestFailure StdGen -> Either TestFailure ()
discardGenerator (Left failure) = Left failure
discardGenerator (Right _) = Right ()

checks : Either TestFailure ()
checks =
    checkAll
        [
            assertEqual "three subscriptions" 3750 $ lineTotal 3 1250,
            assert "zero quantity is free" $ lineTotal 0 9999 == 0,
            discardGenerator $
                checkProperty
                    "non-negative invoice lines"
                    500
                    nonNegativeLine
                    lineTotalIsNonNegative
                    (mkStdGen 20260905)
        ]

main : Task ()
main =
    case checks of
        Left failure => do
            printLine $ "failed: " <> failureMessage failure
            exitFailure
        | Right _ => printLine "all checks passed"
