import std.text(intToString)

sumList : [Int] -> Int
sumList xs =
    case xs of
        [] => 0
        | y :: ys => y + sumList ys

main : Task ()
main = printLine (intToString (sumList [1, 2, 3, 4]))
