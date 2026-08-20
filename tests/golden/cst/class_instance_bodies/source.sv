class TinyEq a where
    tinyEq : a -> a -> Bool
    default tinyNotEq x y =
        if tinyEq x y then False else True

instance TinyEq Int where
    tinyEq 0 0 = True
    tinyEq (x) _ = x == 0
    tinyEq _ y = y == 0

main = tinyEq 1 1
