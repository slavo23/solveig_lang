-- | Adds one to an integer.
documentedIncrement : Int -> Int
documentedIncrement n = n + 1

-- | Holds a documented integer.
data Documented =
    -- | Wraps a documented integer.
    MkDocumented Int

-- | Describes values.
class Describe a where
    -- | Converts a value into a friendly label.
    describe : a -> String
