type ExternalScores = (String, [Int])
type ExternalEq a = Eq a => a -> a -> Bool

externalScores : ExternalScores
externalScores = ("external", [4, 5])

externalSame : forall a. ExternalEq a
externalSame x y = x == y
