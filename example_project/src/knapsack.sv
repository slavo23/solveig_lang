data WTree = Empty | Node Int WTree WTree;
data Score = Score Int Int;

max2 : Int -> Int -> Int;
max2 a b =
    if a < b then b else a;

treeDp : WTree -> Score;
treeDp tree =
    case tree of
        Empty => Score 0 0
      | Node weight left right =>
            case treeDp left of
                Score leftTake leftSkip =>
                    case treeDp right of
                        Score rightTake rightSkip =>
                            let takeHere = weight + leftSkip + rightSkip
                            in
                            let skipHere = (max2 leftTake leftSkip) + (max2 rightTake rightSkip)
                            in
                            Score takeHere skipHere;

maxIndependentSet : WTree -> Int;
maxIndependentSet tree =
    case treeDp tree of
        Score takeHere skipHere => max2 takeHere skipHere;

makeTree : Int -> Int -> WTree;
makeTree depth seed =
    if depth == 0 then
        Empty
    else
        let weight = 1 + (mod ((seed * 17) + 11) 97)
        in
        let leftSeed = (seed * 2) + 1
        in
        let rightSeed = (seed * 2) + 2
        in
        Node weight (makeTree (depth - 1) leftSeed) (makeTree (depth - 1) rightSeed);

nodeCount : WTree -> Int;
nodeCount tree =
    case tree of
        Empty => 0
      | Node _ left right => 1 + nodeCount left + nodeCount right;

benchTreeDp : Int -> Int;
benchTreeDp depth =
    maxIndependentSet (makeTree depth 1);

sampleTreeDp : Int;
sampleTreeDp =
    benchTreeDp 10;
