import aliases(ExternalScores, externalScores, externalSame)

type Ints = [Int]
type ScoreBox a = (String, a)
type Scores = ScoreBox Ints
type EqFn a = Eq a => a -> a -> Bool

sameViaAlias : forall a. EqFn a
sameViaAlias x y = x == y

makeScores : Scores
makeScores = ("scores", [1, 2, 3])

useExternal : ExternalScores
useExternal = externalScores

main : ((Scores, Bool), (ExternalScores, Bool))
main = ((makeScores, (sameViaAlias 7 7)), (useExternal, (externalSame 8 8)))
