export (TestFailure(..), assert, assertEqual, checkAll, failureMessage)

-- | A failed assertion with a stable user-provided label and optional detail.
data TestFailure = TestFailure String

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

-- | Returns the first failed assertion from a deterministic list.
checkAll : [Either TestFailure ()] -> Either TestFailure ()
checkAll [] = Right ()
checkAll (Right _ :: rest) = checkAll rest
checkAll (Left failure :: _) = Left failure

-- | Returns a failed assertion's human-readable message.
failureMessage : TestFailure -> String
failureMessage (TestFailure message) = message
