import prelude.builtins
import prelude.list
import prelude.prim

infix 4 ==, /=, <, <=, >, >=
infixl 6 +, -
infixl 7 *, /

-- | The result of comparing two values.
data Ordering =
    -- | The first value is less than the second.
    LT
  | -- | The two values compare as equal.
    EQ
  | -- | The first value is greater than the second.
    GT

-- | Values that can be checked for equality.
class Eq a where
    -- | Checks whether two values are equal.
    (==) : a -> a -> Bool
    -- | Checks whether two values are different.
    (/=) : a -> a -> Bool
    default (/=) x y = not (x == y)

-- | Named form of equality.
eq : forall a. Eq a => a -> a -> Bool
eq x y = x == y

-- | Named form of inequality.
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

-- | Values that can be ordered.
class Eq a => Ord a where
    -- | Compares two values and returns their ordering.
    compare : a -> a -> Ordering
    -- | Checks whether the first value is less than the second.
    (<) : a -> a -> Bool
    -- | Checks whether the first value is less than or equal to the second.
    (<=) : a -> a -> Bool
    -- | Checks whether the first value is greater than the second.
    (>) : a -> a -> Bool
    -- | Checks whether the first value is greater than or equal to the second.
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

-- | Named form of less-than comparison.
less : forall a. Ord a => a -> a -> Bool
less x y = x < y

-- | Named form of less-than-or-equal comparison.
lessEq : forall a. Ord a => a -> a -> Bool
lessEq x y = x <= y

-- | Named form of greater-than comparison.
greater : forall a. Ord a => a -> a -> Bool
greater x y = x > y

-- | Named form of greater-than-or-equal comparison.
greaterEq : forall a. Ord a => a -> a -> Bool
greaterEq x y = x >= y

-- | Returns the smaller of two ordered values.
minOrd : forall a. Ord a => a -> a -> a
minOrd x y = if x <= y then x else y

-- | Returns the larger of two ordered values.
maxOrd : forall a. Ord a => a -> a -> a
maxOrd x y = if x <= y then y else x

-- | Restricts a value to the inclusive range between low and high.
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

-- | Values that support basic numeric arithmetic.
class Num a where
    -- | Adds two numbers.
    (+) : a -> a -> a
    -- | Subtracts the second number from the first.
    (-) : a -> a -> a
    -- | Multiplies two numbers.
    (*) : a -> a -> a
    -- | Negates a number.
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

-- | Divides two integers using integer division.
div : Int -> Int -> Int
div x y = intDiv x y

-- | Returns the integer remainder after division.
mod : Int -> Int -> Int
mod x y = intMod x y

-- | Subtracts the first argument from the second.
subtract : forall a. Num a => a -> a -> a
subtract x y = y - x

-- | Numeric values that support division.
class Num a => Fractional a where
    -- | Divides one fractional value by another.
    (/) : a -> a -> a

instance Fractional Float where
    (/) x y = floatDiv x y

-- | Type constructors whose contents can be transformed.
class Functor f where
    -- | Applies a function inside a context.
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

instance Functor (IO e) where
    fmap f action =
        prelude.builtins.bindIO action (fun value =>
            prelude.builtins.pureIO (f value))

-- | Functors that can lift values and apply wrapped functions.
class Functor f => Applicative f where
    -- | Lifts a plain value into a context.
    pure : forall a. a -> f a
    -- | Applies a wrapped function to a wrapped value.
    apply : forall a b. f (a -> b) -> f a -> f b

-- | Combines two contextual values with a binary function.
liftA2 : forall f a b c. Applicative f => (a -> b -> c) -> f a -> f b -> f c
liftA2 f fx fy = apply (fmap f fx) fy

-- | Runs two contextual values and keeps the result of the second.
applyRight : forall f a b. Applicative f => f a -> f b -> f b
applyRight left right = liftA2 (fun x => fun y => y) left right

-- | Runs two contextual values and keeps the result of the first.
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

instance Applicative (IO e) where
    pure value = prelude.builtins.pureIO value

    apply actionF actionX =
        prelude.builtins.bindIO actionF (fun f =>
            prelude.builtins.bindIO actionX (fun x =>
                prelude.builtins.pureIO (f x)))

-- | Applicative contexts that can sequence dependent computations.
class Applicative m => Monad m where
    -- | Sequences a computation and passes its result to the next computation.
    bind : forall a b. m a -> (a -> m b) -> m b

-- | Runs two monadic computations and keeps the result of the second.
thenM : forall m a b. Monad m => m a -> m b -> m b
thenM mx my = bind mx (fun x => my)

-- | Flattens one layer of monadic nesting.
join : forall m a. Monad m => m (m a) -> m a
join mmx = do
    mx <- mmx
    mx

-- | Composes two functions that return monadic results.
composeK : forall m a b c. Monad m => (a -> m b) -> (b -> m c) -> a -> m c
composeK f g x = do
    y <- f x
    g y

-- | Maps a function that returns monadic results over a list.
mapM : forall m a b. Monad m => (a -> m b) -> [a] -> m [b]
mapM f [] = pure []
mapM f (y :: ys) = do
    z <- f y
    zs <- mapM f ys
    pure (z :: zs)

-- | Turns a list of monadic computations into one computation producing a list.
sequence : forall m a. Monad m => [m a] -> m [a]
sequence xs = mapM (fun x => x) xs

-- | Filters a list with a predicate that returns monadic results.
filterM : forall m a. Monad m => (a -> m Bool) -> [a] -> m [a]
filterM p [] = pure []
filterM p (y :: ys) = do
    keep <- p y
    zs <- filterM p ys
    if keep then pure (y :: zs) else pure zs

instance Monad Maybe where
    bind mx f =
        case mx of
            Nothing => Nothing
            | Just x => f x

instance Monad List where
    bind xs f = concat (map f xs)

instance Monad (IO e) where
    bind action f = prelude.builtins.bindIO action f

-- | Reads the command-line arguments passed to the program.
getArgs : forall e. IO e [String]
getArgs = prelude.prim.getArgs

-- | Returns the runtime environment sorted by variable name.
getEnvironment : Task [Pair String String]
getEnvironment = prelude.prim.getEnvironment

-- | Lists a directory's immediate entries in lexical order.
listDirectory : String -> Task [String]
listDirectory path = prelude.prim.listDirectory path

-- | Runs two IO actions and keeps the result of the second.
seqIO : forall e a b. IO e a -> IO e b -> IO e b
seqIO left right = thenM left right

-- | Runs an IO action for every element of a list and discards the results.
mapIO_ : forall e a. (a -> IO e ()) -> [a] -> IO e ()
mapIO_ f [] = prelude.builtins.pureIO ()
mapIO_ f (y :: ys) = do
    f y
    mapIO_ f ys

-- | Values that can be rendered as text.
class Show a where
    -- | Renders a value as a string.
    show : a -> String

-- | Escapes a character for display inside character literals.
showCharPayload : Char -> String
showCharPayload c =
    if c == '\n' then "\\n"
    else if c == '\t' then "\\t"
    else if c == '\\' then "\\\\"
    else if c == '\'' then "\\'"
    else if c == '"' then "\\\""
    else charToString c

-- | Escapes a character for display inside string literals.
showStringPayload : Char -> String
showStringPayload c =
    if c == '\n' then "\\n"
    else if c == '\t' then "\\t"
    else if c == '\\' then "\\\\"
    else if c == '"' then "\\\""
    else charToString c

-- | Renders the characters inside a string literal.
showStringChars : [Char] -> String
showStringChars [] = ""
showStringChars (c :: rest) = stringAppend (showStringPayload c) (showStringChars rest)

-- | Renders the comma-separated contents of a list.
showListItems : forall a. Show a => [a] -> String
showListItems [] = ""
showListItems (y :: []) = show y
showListItems (y :: ys) = stringAppend (show y) (stringAppend ", " (showListItems ys))

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
    show value =
        case value of
            True => "True"
            | False => "False"

instance Show Ordering where
    show value =
        case value of
            LT => "LT"
            | EQ => "EQ"
            | GT => "GT"

instance Show a => Show (Maybe a) where
    show value =
        case value of
            Nothing => "Nothing"
            | Just x => stringAppend "Just " (show x)

instance Show a => Show (List a) where
    show value = stringAppend "[" (stringAppend (showListItems value) "]")

instance (Show a, Show b) => Show (Pair a b) where
    show value =
        case value of
            MkPair a b => stringAppend "(" (stringAppend (show a) (stringAppend ", " (stringAppend (show b) ")")))

-- | Values that can be combined associatively.
class Semigroup a where
    -- | Combines two values.
    appendS : a -> a -> a

-- | Semigroups that also provide an empty value.
class Semigroup a => Monoid a where
    -- | The identity value for appendS.
    empty : a

-- | Combines all values in a list.
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
