import prelude.builtins
import prelude.list

data Ordering = LT | EQ | GT

class Eq a where
    eq : a -> a -> Bool

notEq : forall a. Eq a => a -> a -> Bool
notEq x y = not (eq x y)

instance Eq Int where
    eq a b = a == b

instance Eq Bool where
    eq a b =
        case MkPair a b of
            MkPair True True => True
            | MkPair False False => True
            | MkPair _ _ => False

instance Eq a => Eq (Maybe a) where
    eq x y =
        case x of
            Nothing =>
                case y of
                    Nothing => True
                    | Just _ => False
            | Just x1 =>
                case y of
                    Nothing => False
                    | Just y1 => eq x1 y1

instance Eq a => Eq (List a) where
    eq xs ys =
        case xs of
            Nil =>
                case ys of
                    Nil => True
                    | Cons _ _ => False
            | Cons x xsRest =>
                case ys of
                    Nil => False
                    | Cons y ysRest =>
                        if eq x y then eq xsRest ysRest else False

instance (Eq a, Eq b) => Eq (Pair a b) where
    eq left right =
        case left of
            MkPair leftA leftB =>
                case right of
                    MkPair rightA rightB =>
                        if eq leftA rightA then eq leftB rightB else False

class Eq a => Ord a where
    compare : a -> a -> Ordering

less : forall a. Ord a => a -> a -> Bool
less x y =
    case compare x y of
        LT => True
        | EQ => False
        | GT => False

lessEq : forall a. Ord a => a -> a -> Bool
lessEq x y =
    case compare x y of
        LT => True
        | EQ => True
        | GT => False

greater : forall a. Ord a => a -> a -> Bool
greater x y = less y x

greaterEq : forall a. Ord a => a -> a -> Bool
greaterEq x y = lessEq y x

minOrd : forall a. Ord a => a -> a -> a
minOrd x y = if lessEq x y then x else y

maxOrd : forall a. Ord a => a -> a -> a
maxOrd x y = if lessEq x y then y else x

clampOrd : forall a. Ord a => a -> a -> a -> a
clampOrd low high x =
    if less x low then low
    else if less high x then high
    else x

instance Ord Int where
    compare x y =
        if x < y then LT
        else if x > y then GT
        else EQ

instance Ord Bool where
    compare x y =
        case MkPair x y of
            MkPair False False => EQ
            | MkPair False True => LT
            | MkPair True False => GT
            | MkPair True True => EQ

instance Ord a => Ord (Maybe a) where
    compare x y =
        case x of
            Nothing =>
                case y of
                    Nothing => EQ
                    | Just _ => LT
            | Just x1 =>
                case y of
                    Nothing => GT
                    | Just y1 => compare x1 y1

instance Ord a => Ord (List a) where
    compare xs ys =
        case xs of
            Nil =>
                case ys of
                    Nil => EQ
                    | Cons _ _ => LT
            | Cons x xsRest =>
                case ys of
                    Nil => GT
                    | Cons y ysRest =>
                        case compare x y of
                            LT => LT
                            | GT => GT
                            | EQ => compare xsRest ysRest

instance (Ord a, Ord b) => Ord (Pair a b) where
    compare left right =
        case left of
            MkPair leftA leftB =>
                case right of
                    MkPair rightA rightB =>
                        case compare leftA rightA of
                            LT => LT
                            | GT => GT
                            | EQ => compare leftB rightB

class Functor f where
    fmap : forall a b. (a -> b) -> f a -> f b

instance Functor Maybe where
    fmap f mx =
        case mx of
            Nothing => Nothing
            | Just x => Just (f x)

instance Functor List where
    fmap f xs = map f xs

instance Functor (Pair e) where
    fmap f pair =
        case pair of
            MkPair e x => MkPair e (f x)

class Functor f => Applicative f where
    pure : forall a. a -> f a
    apply : forall a b. f (a -> b) -> f a -> f b

liftA2 : forall f a b c. Applicative f => (a -> b -> c) -> f a -> f b -> f c
liftA2 f fx fy = apply (fmap f fx) fy

applyRight : forall f a b. Applicative f => f a -> f b -> f b
applyRight left right = liftA2 (fun x => fun y => y) left right

applyLeft : forall f a b. Applicative f => f a -> f b -> f a
applyLeft left right = liftA2 (fun x => fun y => x) left right

instance Applicative Maybe where
    pure x = Just x

    apply mf mx =
        case mf of
            Nothing => Nothing
            | Just f => fmap f mx

instance Applicative List where
    pure x = [x]

    apply fs xs = concat (map (fun f => map f xs) fs)

class Applicative m => Monad m where
    bind : forall a b. m a -> (a -> m b) -> m b

thenM : forall m a b. Monad m => m a -> m b -> m b
thenM mx my = bind mx (fun x => my)

join : forall m a. Monad m => m (m a) -> m a
join mmx = bind mmx (fun mx => mx)

composeK : forall m a b c. Monad m => (a -> m b) -> (b -> m c) -> a -> m c
composeK f g x = bind (f x) g

mapM : forall m a b. Monad m => (a -> m b) -> [a] -> m [b]
mapM f xs =
    case xs of
        [] => pure []
        | y :: ys =>
            bind (f y) (fun z =>
                bind (mapM f ys) (fun zs =>
                    pure (z :: zs)))

sequence : forall m a. Monad m => [m a] -> m [a]
sequence xs = mapM (fun x => x) xs

filterM : forall m a. Monad m => (a -> m Bool) -> [a] -> m [a]
filterM p xs =
    case xs of
        [] => pure []
        | y :: ys =>
            bind (p y) (fun keep =>
                bind (filterM p ys) (fun zs =>
                    if keep then pure (y :: zs) else pure zs))

instance Monad Maybe where
    bind mx f =
        case mx of
            Nothing => Nothing
            | Just x => f x

instance Monad List where
    bind xs f = concat (map f xs)

class Semigroup a where
    appendS : a -> a -> a

class Semigroup a => Monoid a where
    empty : a

mconcat : forall a. Monoid a => [a] -> a
mconcat xs = foldr appendS empty xs

instance Semigroup (List a) where
    appendS xs ys = append xs ys

instance Monoid (List a) where
    empty = []

instance (Semigroup a, Semigroup b) => Semigroup (Pair a b) where
    appendS left right =
        case left of
            MkPair leftA leftB =>
                case right of
                    MkPair rightA rightB =>
                        MkPair (appendS leftA rightA) (appendS leftB rightB)

instance (Monoid a, Monoid b) => Monoid (Pair a b) where
    empty = MkPair empty empty
