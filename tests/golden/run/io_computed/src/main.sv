import prelude.prim(intToString)

sumList : [Int] -> Int
sumList xs =
    case xs of
        [] => 0
        | y :: ys => y + sumList ys

main : IO () ()
main = printLine (intToString (sumList [1, 2, 3, 4]))
