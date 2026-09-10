build : Int -> List Int -> List Int
build n acc = if n <= 0 then acc else build (n - 1) (n :: acc)

sum : List Int -> Int -> Int
sum xs acc =
    case xs of
        [] => acc
        | x :: rest => sum rest (acc + x)

main = sum (build 70000 []) 0
