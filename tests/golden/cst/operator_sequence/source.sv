infixr 8 ^^
infixl 6 <+>

(<+>) : Int -> Int -> Int
(<+>) x y = x + y

main = 1 + 2 * 3 ^^ 4 <+> 5
