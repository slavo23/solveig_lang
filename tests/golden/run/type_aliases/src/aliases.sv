type ExternalScores = Pair String [Int]
type ExternalEq a = Eq a => a -> a -> Bool

externalScores : ExternalScores
externalScores = MkPair "external" [4, 5]

externalSame : forall a. ExternalEq a
externalSame x y = x == y
