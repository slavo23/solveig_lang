class Score a where
    score : a -> Int

data Box = Box (Int, (Int, Int))

instance Score Box where
    score (Box (0, (left, right))) = left + right
    score (Box (outer, (left, right))) = outer + left + right

scorePair : (Int, Int) -> Int
scorePair (0, right) = right
scorePair (left, right) = left + right

sum2 : (Int, Int) -> Int
sum2 (a, b) = a + b

sum3 : (Int, Int, Int) -> Int
sum3 (a, b, c) = a + b + c

sum4 : (Int, Int, Int, Int) -> Int
sum4 (a, b, c, d) = a + b + c + d

sum5 : (Int, Int, Int, Int, Int) -> Int
sum5 (a, b, c, d, e) = a + b + c + d + e

sum6 : (Int, Int, Int, Int, Int, Int) -> Int
sum6 (a, b, c, d, e, f) = a + b + c + d + e + f

sum7 : (Int, Int, Int, Int, Int, Int, Int) -> Int
sum7 (a, b, c, d, e, f, g) = a + b + c + d + e + f + g

sum8 : (Int, Int, Int, Int, Int, Int, Int, Int) -> Int
sum8 (a, b, c, d, e, f, g, h) = a + b + c + d + e + f + g + h

sumNested : (Int, (Int, Int)) -> Int
sumNested (outer, (left, right)) = outer + left + right

makeTriple : Int -> Int -> Int -> (Int, Int, Int)
makeTriple = (,,)

main : (Int, Int, Int, Int, Int, Int, Int, Int)
main =
    ( sum2 (1, 2)
    , sum3 (makeTriple 1 2 3)
    , sumNested (1, (2, 3))
    , sum4 (1, 2, 3, 4)
    , sum5 (1, 2, 3, 4, 5)
    , sum6 (1, 2, 3, 4, 5, 6)
    , sum7 (1, 2, 3, 4, 5, 6, 7)
    , sum8 (1, 2, 3, 4, 5, 6, 7, 8)
        + scorePair (4, 5)
        + score (Box (0, (6, 7)))
    )
