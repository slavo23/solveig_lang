import prelude.builtins
import prelude.list

class Functor f where
    fmap : forall a b. (a -> b) -> f a -> f b

instance Functor List where
    fmap f xs = map f xs

instance Functor Maybe where
    fmap f mx = 
        case mx of
            Nothing => Nothing
            | Just j => Just (f j)

class Eq a where
    eq : a -> a -> Bool

instance Eq Int where
    eq a b = a == b

instance Eq Bool where
    eq a b = 
        case MkPair a b of
            MkPair True True => True
            | MkPair False False => True
            | MkPair _ _ => False

