import prelude.builtins
import prelude.list
import prelude.prim

infix 4 ==, /=, <, <=, >, >=
infixl 6 +, -
infixl 7 *, /

data Ordering = LT | EQ | GT

class Eq a where
    (==) : a -> a -> Bool
    (/=) : a -> a -> Bool
    default (/=) x y = not (x == y)

eq : forall a. Eq a => a -> a -> Bool
eq x y = x == y

notEq : forall a. Eq a => a -> a -> Bool
notEq x y = x /= y

instance Eq Int where
    (==) a b = intEq a b

instance Eq Float where
    (==) a b = floatEq a b

instance Eq Char where
    (==) a b = charEq a b

instance Eq String where
    (==) a b = stringEq a b

instance Eq () where
    (==) _ _ = True

instance Eq Bool where
    (==) a b =
        case MkPair a b of
            MkPair True True => True
            | MkPair False False => True
            | MkPair _ _ => False

instance Eq a => Eq (Maybe a) where
    (==) x y =
        case x of
            Nothing =>
                case y of
                    Nothing => True
                    | Just _ => False
            | Just x1 =>
                case y of
                    Nothing => False
                    | Just y1 => x1 == y1

instance Eq a => Eq (List a) where
    (==) xs ys =
        case xs of
            Nil =>
                case ys of
                    Nil => True
                    | Cons _ _ => False
            | Cons x xsRest =>
                case ys of
                    Nil => False
                    | Cons y ysRest =>
                        if x == y then xsRest == ysRest else False

instance (Eq a, Eq b) => Eq (Pair a b) where
    (==) left right =
        case left of
            MkPair leftA leftB =>
                case right of
                    MkPair rightA rightB =>
                        if leftA == rightA then leftB == rightB else False

class Eq a => Ord a where
    compare : a -> a -> Ordering
    (<) : a -> a -> Bool
    (<=) : a -> a -> Bool
    (>) : a -> a -> Bool
    (>=) : a -> a -> Bool
    default (<) x y =
        case compare x y of
            LT => True
            | EQ => False
            | GT => False
    default (<=) x y =
        case compare x y of
            LT => True
            | EQ => True
            | GT => False
    default (>) x y =
        case compare x y of
            LT => False
            | EQ => False
            | GT => True
    default (>=) x y =
        case compare x y of
            LT => False
            | EQ => True
            | GT => True

less : forall a. Ord a => a -> a -> Bool
less x y = x < y

lessEq : forall a. Ord a => a -> a -> Bool
lessEq x y = x <= y

greater : forall a. Ord a => a -> a -> Bool
greater x y = x > y

greaterEq : forall a. Ord a => a -> a -> Bool
greaterEq x y = x >= y

minOrd : forall a. Ord a => a -> a -> a
minOrd x y = if x <= y then x else y

maxOrd : forall a. Ord a => a -> a -> a
maxOrd x y = if x <= y then y else x

clampOrd : forall a. Ord a => a -> a -> a -> a
clampOrd low high x =
    if x < low then low
    else if high < x then high
    else x

instance Ord Int where
    compare x y = intCompare x y

instance Ord Float where
    compare x y =
        if floatLt x y then LT
        else if floatGt x y then GT
        else EQ

instance Ord Char where
    compare x y = charCompare x y

instance Ord String where
    compare x y = stringCompare x y

instance Ord () where
    compare _ _ = EQ

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

class Num a where
    (+) : a -> a -> a
    (-) : a -> a -> a
    (*) : a -> a -> a
    negate : a -> a

instance Num Int where
    (+) x y = intAdd x y
    (-) x y = intSub x y
    (*) x y = intMul x y
    negate x = intNeg x

instance Num Float where
    (+) x y = floatAdd x y
    (-) x y = floatSub x y
    (*) x y = floatMul x y
    negate x = floatNeg x

div : Int -> Int -> Int
div x y = intDiv x y

mod : Int -> Int -> Int
mod x y = intMod x y

class Num a => Fractional a where
    (/) : a -> a -> a

instance Fractional Float where
    (/) x y = floatDiv x y

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

instance Functor IO where
    fmap f action =
        prelude.builtins.bindIO action (fun value =>
            prelude.builtins.pureIO (f value))

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

instance Applicative IO where
    pure value = prelude.builtins.pureIO value

    apply actionF actionX =
        prelude.builtins.bindIO actionF (fun f =>
            prelude.builtins.bindIO actionX (fun x =>
                prelude.builtins.pureIO (f x)))

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

instance Monad IO where
    bind action f = prelude.builtins.bindIO action f

getArgs : IO [String]
getArgs = prelude.prim.getArgs

seqIO : forall a b. IO a -> IO b -> IO b
seqIO left right = prelude.builtins.bindIO left (fun ignored => right)

mapIO_ : forall a. (a -> IO ()) -> [a] -> IO ()
mapIO_ f xs =
    case xs of
        [] => prelude.builtins.pureIO ()
        | y :: ys => prelude.builtins.bindIO (f y) (fun ignored => mapIO_ f ys)

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
