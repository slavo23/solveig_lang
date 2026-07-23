class Eq a => OrdLike a where
    lessLike : a -> a -> Bool

class OrdLike a => BoundedLike a where
    clampLike : a -> a

instance OrdLike Int where
    lessLike x y = x < y

instance BoundedLike Int where
    clampLike x =
        if lessLike x 0 then 0
        else if lessLike 100 x then 100
        else x