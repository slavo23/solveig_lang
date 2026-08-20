import prelude.builtins
import prelude.control
import prelude.list
import prelude.prim
import prelude.show
import prelude.typeclasses

instance Eq Int where
    (==) x y = intEq x y

instance Eq Float where
    (==) x y = floatEq x y

instance Eq Char where
    (==) x y = charEq x y

instance Eq String where
    (==) x y = stringEq x y

instance Eq () where
    (==) _ _ = True

instance Eq Bool where
    (==) True True = True
    (==) False False = True
    (==) _ _ = False

instance Eq Ordering where
    (==) LT LT = True
    (==) EQ EQ = True
    (==) GT GT = True
    (==) _ _ = False

instance Eq a => Eq (Maybe a) where
    (==) Nothing Nothing = True
    (==) (Just x) (Just y) = x == y
    (==) _ _ = False

instance Eq a => Eq (List a) where
    (==) [] [] = True
    (==) (x :: xs) (y :: ys) = if x == y then xs == ys else False
    (==) _ _ = False

instance (Eq a, Eq b) => Eq (Pair a b) where
    (==) (MkPair leftA leftB) (MkPair rightA rightB) =
        if leftA == rightA then leftB == rightB else False

instance (Eq a, Eq b) => Eq (Either a b) where
    (==) (Left x) (Left y) = x == y
    (==) (Right x) (Right y) = x == y
    (==) _ _ = False

instance (Eq e, Eq a) => Eq (Result e a) where
    (==) (Ok x) (Ok y) = x == y
    (==) (Err x) (Err y) = x == y
    (==) _ _ = False

instance PartialOrd Int where
    partialCompare x y = Just (intCompare x y)

instance PartialOrd Float where
    partialCompare x y =
        if floatLt x y then Just LT
        else if floatEq x y then Just EQ
        else if floatGt x y then Just GT
        else Nothing

instance PartialOrd Char where
    partialCompare x y = Just (charCompare x y)

instance PartialOrd String where
    partialCompare x y = Just (stringCompare x y)

instance PartialOrd () where
    partialCompare _ _ = Just EQ

instance PartialOrd Bool where
    partialCompare False False = Just EQ
    partialCompare False True = Just LT
    partialCompare True False = Just GT
    partialCompare True True = Just EQ

instance PartialOrd Ordering where
    partialCompare LT LT = Just EQ
    partialCompare LT _ = Just LT
    partialCompare EQ LT = Just GT
    partialCompare EQ EQ = Just EQ
    partialCompare EQ GT = Just LT
    partialCompare GT GT = Just EQ
    partialCompare GT _ = Just GT

instance PartialOrd a => PartialOrd (Maybe a) where
    partialCompare Nothing Nothing = Just EQ
    partialCompare Nothing (Just y) = Just LT
    partialCompare (Just x) Nothing = Just GT
    partialCompare (Just x) (Just y) = partialCompare x y

instance PartialOrd a => PartialOrd (List a) where
    partialCompare [] [] = Just EQ
    partialCompare [] (y :: ys) = Just LT
    partialCompare (x :: xs) [] = Just GT
    partialCompare (x :: xs) (y :: ys) =
        case partialCompare x y of
            Nothing => Nothing
            | Just LT => Just LT
            | Just GT => Just GT
            | Just EQ => partialCompare xs ys

instance (PartialOrd a, PartialOrd b) => PartialOrd (Pair a b) where
    partialCompare (MkPair leftA leftB) (MkPair rightA rightB) =
        case partialCompare leftA rightA of
            Nothing => Nothing
            | Just LT => Just LT
            | Just GT => Just GT
            | Just EQ => partialCompare leftB rightB

instance (PartialOrd a, PartialOrd b) => PartialOrd (Either a b) where
    partialCompare (Left x) (Left y) = partialCompare x y
    partialCompare (Left x) (Right y) = Just LT
    partialCompare (Right x) (Left y) = Just GT
    partialCompare (Right x) (Right y) = partialCompare x y

instance (PartialOrd e, PartialOrd a) => PartialOrd (Result e a) where
    partialCompare (Ok x) (Ok y) = partialCompare x y
    partialCompare (Ok x) (Err y) = Just LT
    partialCompare (Err x) (Ok y) = Just GT
    partialCompare (Err x) (Err y) = partialCompare x y

instance Ord Int where
    compare x y = intCompare x y

instance Ord Char where
    compare x y = charCompare x y

instance Ord String where
    compare x y = stringCompare x y

instance Ord () where
    compare _ _ = EQ

instance Ord Bool where
    compare False False = EQ
    compare False True = LT
    compare True False = GT
    compare True True = EQ

instance Ord Ordering where
    compare LT LT = EQ
    compare LT _ = LT
    compare EQ LT = GT
    compare EQ EQ = EQ
    compare EQ GT = LT
    compare GT GT = EQ
    compare GT _ = GT

instance Ord a => Ord (Maybe a) where
    compare Nothing Nothing = EQ
    compare Nothing (Just y) = LT
    compare (Just x) Nothing = GT
    compare (Just x) (Just y) = compare x y

instance Ord a => Ord (List a) where
    compare [] [] = EQ
    compare [] (y :: ys) = LT
    compare (x :: xs) [] = GT
    compare (x :: xs) (y :: ys) =
        case compare x y of
            LT => LT
            | GT => GT
            | EQ => compare xs ys

instance (Ord a, Ord b) => Ord (Pair a b) where
    compare (MkPair leftA leftB) (MkPair rightA rightB) =
        case compare leftA rightA of
            LT => LT
            | GT => GT
            | EQ => compare leftB rightB

instance (Ord a, Ord b) => Ord (Either a b) where
    compare (Left x) (Left y) = compare x y
    compare (Left x) (Right y) = LT
    compare (Right x) (Left y) = GT
    compare (Right x) (Right y) = compare x y

instance (Ord e, Ord a) => Ord (Result e a) where
    compare (Ok x) (Ok y) = compare x y
    compare (Ok x) (Err y) = LT
    compare (Err x) (Ok y) = GT
    compare (Err x) (Err y) = compare x y

instance Num Int where
    (+) x y = intAdd x y
    (*) x y = intMul x y
    negate x = intNeg x

instance Num Float where
    (+) x y = floatAdd x y
    (*) x y = floatMul x y
    negate x = floatNeg x

instance Fractional Float where
    (/) x y = floatDiv x y

instance Functor Maybe where
    fmap f Nothing = Nothing
    fmap f (Just x) = Just (f x)

instance Functor List where
    fmap f xs = map f xs

instance Functor (Pair e) where
    fmap f (MkPair e x) = MkPair e (f x)

instance Functor (Either e) where
    fmap f (Left error) = Left error
    fmap f (Right x) = Right (f x)

instance Functor (Result e) where
    fmap f (Ok x) = Ok (f x)
    fmap f (Err error) = Err error

instance Functor (IO e) where
    fmap f action =
        prelude.builtins.bindIO action (fun value =>
            prelude.builtins.pureIO (f value))

instance Applicative Maybe where
    pure x = Just x
    apply Nothing mx = Nothing
    apply (Just f) mx = fmap f mx

instance Applicative List where
    pure x = [x]
    apply fs xs = concat (map (fun f => map f xs) fs)

instance Applicative (Either e) where
    pure x = Right x
    apply (Left error) mx = Left error
    apply (Right f) mx = fmap f mx

instance Applicative (Result e) where
    pure x = Ok x
    apply (Err error) mx = Err error
    apply (Ok f) mx = fmap f mx

instance Applicative (IO e) where
    pure value = prelude.builtins.pureIO value
    apply actionF actionX =
        prelude.builtins.bindIO actionF (fun f =>
            prelude.builtins.bindIO actionX (fun x =>
                prelude.builtins.pureIO (f x)))

instance Monad Maybe where
    bind Nothing f = Nothing
    bind (Just x) f = f x

instance Monad List where
    bind xs f = concat (map f xs)

instance Monad (Either e) where
    bind (Left error) f = Left error
    bind (Right x) f = f x

instance Monad (Result e) where
    bind (Err error) f = Err error
    bind (Ok x) f = f x

instance Monad (IO e) where
    bind action f = prelude.builtins.bindIO action f

instance Show Int where
    show value = intToString value

instance Show Float where
    show value = floatToString value

instance Show Char where
    show value = stringAppend "'" (stringAppend (showCharPayload value) "'")

instance Show String where
    show value = stringAppend "\"" (stringAppend (showStringChars (stringToChars value)) "\"")

instance Show () where
    show value = "()"

instance Show Bool where
    show True = "True"
    show False = "False"

instance Show Ordering where
    show LT = "LT"
    show EQ = "EQ"
    show GT = "GT"

instance Show a => Show (Maybe a) where
    show Nothing = "Nothing"
    show (Just x) = stringAppend "Just " (show x)

instance Show a => Show (List a) where
    show value = stringAppend "[" (stringAppend (showListItems value) "]")

instance (Show a, Show b) => Show (Pair a b) where
    show (MkPair a b) =
        stringAppend "(" (stringAppend (show a) (stringAppend ", " (stringAppend (show b) ")")))

instance (Show a, Show b) => Show (Either a b) where
    show (Left x) = stringAppend "Left " (show x)
    show (Right x) = stringAppend "Right " (show x)

instance (Show e, Show a) => Show (Result e a) where
    show (Ok x) = stringAppend "Ok " (show x)
    show (Err error) = stringAppend "Err " (show error)

instance Semigroup String where
    combine x y = stringAppend x y

instance Monoid String where
    empty = ""

instance Semigroup (List a) where
    combine xs ys = append xs ys

instance Monoid (List a) where
    empty = []

instance (Semigroup a, Semigroup b) => Semigroup (Pair a b) where
    combine (MkPair leftA leftB) (MkPair rightA rightB) =
        MkPair (combine leftA rightA) (combine leftB rightB)

instance (Monoid a, Monoid b) => Monoid (Pair a b) where
    empty = MkPair empty empty
