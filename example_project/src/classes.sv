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

useEq : forall a. Eq a => a -> a -> Bool
useEq x y = eq x y

eqIntExample : Bool
eqIntExample = eq 1 2

eqBoolExample : Bool
eqBoolExample = eq True True
