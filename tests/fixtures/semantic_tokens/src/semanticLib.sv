-- | External declarations for semantic-token coverage.
data Documented = MkDocumented Int

class Describe a where
    describe : a -> String

documentedIncrement : Int -> Int
documentedIncrement n = n + 1
