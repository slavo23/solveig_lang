infixr 8 ^^
infixl 7 .
infixl 6 <+>

(<+>) : Int -> Int -> Int
(<+>) x y = x * 10 + y

(^^) : Int -> Int -> Int
(^^) x y = x * 100 + y

(.) : Int -> Int -> Int
(.) x y = x * 10 + y

(%%) : Int -> Int -> Int
(%%) x y = x * 10 + y

sumSmall : [Int] -> Int
sumSmall xs =
    case xs of
        [] => 0
        | x :: rest => x + sumSmall rest

leftBuiltIn = 20 - 5 - 3
rightDeclared = 1 ^^ 2 ^^ 3
dotOperator = 2 . 3
defaultLeft = 1 %% 2 %% 3
parenOp = (<+>) 4 5
consSum = sumSmall (1 :: 2 :: [])

main = leftBuiltIn + rightDeclared + dotOperator + defaultLeft + parenOp + consSum
