sumSmall : [Int] -> Int
sumSmall xs =
    case xs of
        [] => 0
        | x :: rest => x + sumSmall rest

main = let total = sumSmall [1, 2, 3, 4] in if total == 10 then 42 else 0
