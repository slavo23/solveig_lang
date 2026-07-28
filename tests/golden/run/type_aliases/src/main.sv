import aliases(ExternalScores, externalScores, externalSame)

type Ints = [Int]
type ScoreBox a = Pair String a
type Scores = ScoreBox Ints
type EqFn a = Eq a => a -> a -> Bool

sameViaAlias : forall a. EqFn a
sameViaAlias x y = x == y

makeScores : Scores
makeScores = MkPair "scores" [1, 2, 3]

useExternal : ExternalScores
useExternal = externalScores

main : Pair (Pair Scores Bool) (Pair ExternalScores Bool)
main = MkPair
    (MkPair makeScores (sameViaAlias 7 7))
    (MkPair useExternal (externalSame 8 8))
