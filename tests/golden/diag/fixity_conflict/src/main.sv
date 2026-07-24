infixl 6 <+>
infixr 6 <^>

(<+>) : Int -> Int -> Int
(<+>) x y = x + y

(<^>) : Int -> Int -> Int
(<^>) x y = x + y

main = 1 <+> 2 <^> 3
