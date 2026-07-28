-- | Top-level docs.
main = 1 -- ^ trailing docs

-- | Type docs.
data Documented =
    -- | Constructor docs.
    MkDocumented Int

-- | Class docs.
class Describe a where
    -- | Method docs.
    describe : a -> String
