import std.list
data Split a = Split [a] [a];

reverseAppend : forall a. [a] -> [a] -> [a];
reverseAppend rev rest =
    case rev of
        [] => rest
      | x :: xs => reverseAppend xs (x :: rest);

mergeRevBy : forall a. (a -> a -> Bool) -> [a] -> [a] -> [a] -> [a];
mergeRevBy lt xs ys acc =
    case xs of
        [] => reverseAppend acc ys
      | x :: xsRest =>
            case ys of
                [] => reverseAppend acc (x :: xsRest)
              | y :: ysRest =>
                    if lt x y then mergeRevBy lt xsRest ys (x :: acc)
                    else if lt y x then mergeRevBy lt xs ysRest (y :: acc)
                    else mergeRevBy lt xsRest ys (x :: acc)
            

mergeBy : forall a. (a -> a -> Bool) -> [a] -> [a] -> [a]
mergeBy lt xs ys =
    mergeRevBy lt xs ys []

splitAlt : forall a. [a] -> [a] -> [a] -> Split a
splitAlt xs left right =
    case xs of
        [] => Split left right
      | x :: [] => Split (x :: left) right
      | x :: y :: zs => splitAlt zs (x :: left) (y :: right)

mergesortBy : forall a. (a -> a -> Bool) -> [a] -> [a]
mergesortBy lt xs =
    case xs of
        [] => []
      | x :: [] => x :: []
      | _ =>
            case splitAlt xs [] [] of
                Split left right =>
                    mergeBy lt (mergesortBy lt left) (mergesortBy lt right)
            

ltInt : Int -> Int -> Bool
ltInt x y =
    x < y;

mergesortInt : [Int] -> [Int]
mergesortInt xs =
    mergesortBy ltInt xs;

sumList : [Int] -> Int;
sumList xs =
    case xs of
        [] => 0
      | x :: xsRest => x + sumList xsRest

makeInput : Int -> [Int];
makeInput n =
    reverse (rangeInclusive 1 n)

benchMergesort : Int -> Int;
benchMergesort n =
    sumList (mergesortInt (makeInput n))

sampleSortedMergeSort : Int -> [Int]
sampleSortedMergeSort n =
    mergesortInt (makeInput n)
