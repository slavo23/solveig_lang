export (
    TestFailure(..), Gen, assert, assertEqual, assertIO, assertWithResource,
    checkAll, failureMessage, generate, constantGen, mapGen, pairGen, chooseInt,
    listOf, checkProperty
)

private (drawInt, drawList, drawListInto, checkPropertyFrom)

import std.list(reverse)
import std.random(randomRange)
import std.task(bracket)

-- | A failed assertion with a stable user-provided label and optional detail.
data TestFailure = TestFailure String

-- | A pure random value generator that threads immutable StdGen state.
data Gen a = Gen (StdGen -> (a, StdGen))

instance Eq TestFailure where
    (==) (TestFailure left) (TestFailure right) = left == right

instance Show TestFailure where
    show (TestFailure message) = message

-- | Checks a Boolean condition without raising a defect.
assert : String -> Bool -> Either TestFailure ()
assert label condition =
    if condition then Right () else Left (TestFailure label)

-- | Checks equality and records both displayed values when they differ.
assertEqual : forall a. (Eq a, Show a) => String -> a -> a -> Either TestFailure ()
assertEqual label expected actual =
    if expected == actual then Right ()
    else Left
        (TestFailure
            (label <> ": expected " <> show expected <> ", got " <> show actual))

-- | Checks a Boolean IO result while preserving the action's typed error.
assertIO : forall e. String -> IO e Bool -> IO e (Either TestFailure ())
assertIO label = fmap (assert label)

-- | Acquires a resource, checks it, and releases it on every scope exit path.
assertWithResource : forall e resource cleanup. String -> IO e resource -> (resource -> IO e cleanup) -> (resource -> IO e Bool) -> IO e (Either TestFailure ())
assertWithResource label acquire release inspect =
    bracket acquire release (fun resource => assertIO label (inspect resource))

-- | Returns the first failed assertion from a deterministic list.
checkAll : [Either TestFailure ()] -> Either TestFailure ()
checkAll [] = Right ()
checkAll (Right _ :: rest) = checkAll rest
checkAll (Left failure :: _) = Left failure

-- | Returns a failed assertion's human-readable message.
failureMessage : TestFailure -> String
failureMessage (TestFailure message) = message

-- | Draws one value and returns the next explicit generator state.
generate : forall a. Gen a -> StdGen -> (a, StdGen)
generate (Gen draw) = draw

-- | Generates one fixed value without advancing the generator state.
constantGen : forall a. a -> Gen a
constantGen value = Gen (fun generator => (value, generator))

-- | Transforms generated values without changing state progression.
mapGen : forall a b. (a -> b) -> Gen a -> Gen b
mapGen transform (Gen draw) =
    Gen (fun generator =>
        case draw generator of
            (value, nextGenerator) => (transform value, nextGenerator))

-- | Generates a pair from left to right while threading one generator state.
pairGen : forall a b. Gen a -> Gen b -> Gen (a, b)
pairGen (Gen drawLeft) (Gen drawRight) =
    Gen (fun generator =>
        case drawLeft generator of
            (left, afterLeft) =>
                case drawRight afterLeft of
                    (right, afterRight) => ((left, right), afterRight))

-- | Builds an unbiased inclusive integer generator when bounds are ordered.
chooseInt : Int -> Int -> Maybe (Gen Int)
chooseInt lower upper =
    if lower > upper then Nothing
    else Just (Gen (drawInt lower upper))

-- | Builds a fixed-length list generator for a non-negative length.
listOf : forall a. Int -> Gen a -> Maybe (Gen [a])
listOf count generator =
    if count < 0 then Nothing
    else Just (Gen (drawList count generator))

-- | Checks a property against an exact positive number of deterministic cases.
-- |
-- | The caller supplies the initial StdGen. Success returns the next state;
-- | failure identifies the one-based case and displayed generated value.
checkProperty : forall a. Show a => String -> Int -> Gen a -> (a -> Bool) -> StdGen -> Either TestFailure StdGen
checkProperty label count generator property initialGenerator =
    if count < 1 then
        Left
            (TestFailure
                (label <> ": property case count must be positive, got " <> show count))
    else checkPropertyFrom label count 1 generator property initialGenerator

drawInt : Int -> Int -> StdGen -> (Int, StdGen)
drawInt lower upper generator =
    case randomRange lower upper generator of
        Nothing => error "validated integer generator has reversed bounds"
        | Just outcome => outcome

drawList : forall a. Int -> Gen a -> StdGen -> ([a], StdGen)
drawList count generator initialGenerator =
    drawListInto count generator initialGenerator []

drawListInto : forall a. Int -> Gen a -> StdGen -> [a] -> ([a], StdGen)
drawListInto remaining generator currentGenerator reversed =
    if remaining == 0 then (reverse reversed, currentGenerator)
    else
        case generate generator currentGenerator of
            (value, nextGenerator) =>
                drawListInto
                    (remaining - 1)
                    generator
                    nextGenerator
                    (value :: reversed)

checkPropertyFrom : forall a. Show a => String -> Int -> Int -> Gen a -> (a -> Bool) -> StdGen -> Either TestFailure StdGen
checkPropertyFrom label remaining caseNumber generator property currentGenerator =
    if remaining == 0 then Right currentGenerator
    else
        case generate generator currentGenerator of
            (value, nextGenerator) =>
                if property value then
                    checkPropertyFrom
                        label
                        (remaining - 1)
                        (caseNumber + 1)
                        generator
                        property
                        nextGenerator
                else
                    Left
                        (TestFailure
                            (label
                                <> ": property failed at case "
                                <> show caseNumber
                                <> " with value "
                                <> show value))
