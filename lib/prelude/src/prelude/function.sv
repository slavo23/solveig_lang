infixr 9 .
infixr 0 $

-- | Returns its argument unchanged.
id : forall a. a -> a
id x = x

-- | Builds a function that always returns its first argument.
const : forall a b. a -> b -> a
const x y = x

-- | Reverses the first two arguments of a function.
flip : forall a b c. (a -> b -> c) -> b -> a -> c
flip f y x = f x y

-- | Composes two functions from right to left.
(.) : forall a b c. (b -> c) -> (a -> b) -> a -> c
(.) f g x = f (g x)

-- | Applies a function with the lowest available precedence.
($) : forall a b. (a -> b) -> a -> b
($) f x = f x
