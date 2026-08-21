-- | A failed assertion with a stable user-provided label and optional detail.
data TestFailure = TestFailure String

-- | Checks a Boolean condition without raising a defect.
assert : String -> Bool -> Result TestFailure ()
assert label condition =
    if condition then Ok () else Err (TestFailure label)

-- | Checks equality and records both displayed values when they differ.
assertEqual : forall a. (Eq a, Show a) => String -> a -> a -> Result TestFailure ()
assertEqual label expected actual =
    if expected == actual then Ok ()
    else Err
        (TestFailure
            (label <> ": expected " <> show expected <> ", got " <> show actual))

-- | Returns the first failed assertion from a deterministic list.
checkAll : [Result TestFailure ()] -> Result TestFailure ()
checkAll [] = Ok ()
checkAll (Ok _ :: rest) = checkAll rest
checkAll (Err failure :: _) = Err failure

-- | Returns a failed assertion's human-readable message.
failureMessage : TestFailure -> String
failureMessage (TestFailure message) = message
