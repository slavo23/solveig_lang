double x = x + x

lessAndSame x y =
    if less x y then eq x y else False

containsLeft target xs =
    case xs of
        [] => False
        | x :: rest =>
            if x == target then True else containsRight target rest

containsRight target xs =
    case xs of
        [] => False
        | x :: rest =>
            if x == target then True else containsLeft target rest

main = double 21
