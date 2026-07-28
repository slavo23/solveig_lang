class TinyEq a where
    tinyEq : a -> a -> Bool
    default tinyNotEq x y =
        if tinyEq x y then False else True

instance TinyEq Int where
    tinyEq x y = x == y

main = tinyEq 1 1
