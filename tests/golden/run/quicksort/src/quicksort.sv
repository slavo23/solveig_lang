import std.list(*)

data Partition3 a = Partition3 [a] [a] [a]

middleFrom : forall a. a -> [a] -> [a] -> a
middleFrom fallback slow fast =
    case fast of
        [] =>
            case slow of
                [] => fallback
                | y :: _ => y
        | _ :: [] =>
            case slow of
                [] => fallback
                | y :: _ => y
        | _ :: _ :: fastRest =>
            case slow of
                [] => fallback
                | _ :: slowRest => middleFrom fallback slowRest fastRest

middleOr : forall a. a -> [a] -> a
middleOr fallback xs = middleFrom fallback xs xs

partition3By : forall a. (a -> a -> Bool) -> a -> [a] -> [a] -> [a] -> [a] -> Partition3 a
partition3By lt pivot xs less equal greater =
    case xs of
        [] => Partition3 less equal greater
        | y :: ys =>
            if lt y pivot
                then partition3By lt pivot ys (y :: less) equal greater
                else if lt pivot y
                        then partition3By lt pivot ys less equal (y :: greater)
                        else partition3By lt pivot ys less (y :: equal) greater

quicksortByGo : forall a. (a -> a -> Bool) -> [a] -> [a] -> [a]
quicksortByGo lt xs acc =
    case xs of
        [] => acc
        | x :: _ =>
            let pivot =
                middleOr x xs
            in case partition3By lt pivot xs [] [] [] of
                Partition3 less equal greater =>
                    quicksortByGo lt less (append (reverse equal) (quicksortByGo lt greater acc))

quicksortBy : forall a. (a -> a -> Bool) -> [a] -> [a]
quicksortBy lt xs = quicksortByGo lt xs []

ltInt : Int -> Int -> Bool
ltInt x y = x < y

quicksortInt : [Int] -> [Int]
quicksortInt xs = quicksortBy ltInt xs

sampleInput : Int -> [Int]
sampleInput howMany = reverse (rangeInclusive 1 howMany)

sampleSortedQuickSort : Int -> [Int]
sampleSortedQuickSort n = quicksortInt (sampleInput n)
