class TinyEq a where
    tinyEq : a -> a -> Bool
    tinyNotEq : a -> a -> Bool
    default tinyNotEq x y = if tinyEq x y then False else True

instance TinyEq Int where
    tinyEq x y = x == y

main : Int
main = if tinyNotEq 1 2 then 77 else 0
