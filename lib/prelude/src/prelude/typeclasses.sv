export (
    Ordering(..), Eq, eq, notEq, PartialOrd, Ord, less, lessEq, greater, greaterEq,
    minOrd, maxOrd, clampOrd, totalCompareFloat, Num, div, mod, subtract, Fractional,
    Functor, (<$>), Applicative, (<*>), liftA2, applyRight, applyLeft, (<*), (*>),
    Monad, (>>=), thenM, (>>), Show, Semigroup, (<>), Monoid, mconcat,
    Foldable, foldMap, Traversable, sequenceA, Hashable, hash
)

import prelude.builtins(*)
import prelude.list(*)
import prelude.prim(*)

infix 4 ==, /=, <, <=, >, >=
infixl 6 +, -
infixl 7 *, /
infixl 4 <$>, <*>, <*, *>
infixl 1 >>=, >>
infixr 6 <>

-- | The result of comparing two values in a total order.
data Ordering =
    -- | The first value is less than the second.
    LT
  | -- | The values compare as equal.
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
eq = (==)

-- | Named form of inequality.
notEq : forall a. Eq a => a -> a -> Bool
notEq = (/=)

-- | Values whose comparison may be unordered.
-- |
-- | Float belongs here because comparisons involving NaN have no mathematical
-- | ordering. Ordinary relational operators therefore remain honest about NaN.
class Eq a => PartialOrd a where
    -- | Compares two values, returning Nothing when they are unordered.
    partialCompare : a -> a -> Maybe Ordering
    -- | Checks whether the first value is less than the second.
    (<) : a -> a -> Bool
    -- | Checks whether the first value is less than or equal to the second.
    (<=) : a -> a -> Bool
    -- | Checks whether the first value is greater than the second.
    (>) : a -> a -> Bool
    -- | Checks whether the first value is greater than or equal to the second.
    (>=) : a -> a -> Bool
    default (<) x y =
        case partialCompare x y of
            Just LT => True
            | _ => False
    default (<=) x y =
        case partialCompare x y of
            Just LT => True
            | Just EQ => True
            | _ => False
    default (>) x y =
        case partialCompare x y of
            Just GT => True
            | _ => False
    default (>=) x y =
        case partialCompare x y of
            Just EQ => True
            | Just GT => True
            | _ => False

-- | Values with a total order.
class PartialOrd a => Ord a where
    -- | Compares two values in their total order.
    compare : a -> a -> Ordering

-- | Named form of less-than comparison.
less : forall a. PartialOrd a => a -> a -> Bool
less = (<)

-- | Named form of less-than-or-equal comparison.
lessEq : forall a. PartialOrd a => a -> a -> Bool
lessEq = (<=)

-- | Named form of greater-than comparison.
greater : forall a. PartialOrd a => a -> a -> Bool
greater = (>)

-- | Named form of greater-than-or-equal comparison.
greaterEq : forall a. PartialOrd a => a -> a -> Bool
greaterEq = (>=)

-- | Returns the smaller of two totally ordered values.
minOrd : forall a. Ord a => a -> a -> a
minOrd x y = if x <= y then x else y

-- | Returns the larger of two totally ordered values.
maxOrd : forall a. Ord a => a -> a -> a
maxOrd x y = if x <= y then y else x

-- | Restricts a value to an inclusive range in a total order.
clampOrd : forall a. Ord a => a -> a -> a -> a
clampOrd low high x =
    if x < low then low
    else if high < x then high
    else x

-- | Gives floating-point values an explicit deterministic IEEE total order.
-- |
-- | Use this for sorting, maps, and reproducible output. Ordinary Float
-- | comparisons still use PartialOrd and report NaN as unordered.
totalCompareFloat : Float -> Float -> Ordering
totalCompareFloat x y = prelude.prim.floatTotalCompare x y

-- | Values that support basic numeric arithmetic.
-- |
-- | The class uses operations that can also be implemented by a future BigInt.
class Num a where
    -- | Adds two numbers.
    (+) : a -> a -> a
    -- | Subtracts the second number from the first.
    (-) : a -> a -> a
    -- | Multiplies two numbers.
    (*) : a -> a -> a
    -- | Negates a number.
    negate : a -> a
    default (-) x y = x + negate y

-- | Divides two integers using integer division.
div : Int -> Int -> Int
div x y = intDiv x y

-- | Returns the integer remainder after division.
mod : Int -> Int -> Int
mod x y = intMod x y

-- | Subtracts the first argument from the second.
subtract : forall a. Num a => a -> a -> a
subtract x y = y - x

-- | Numeric values that support fractional division.
class Num a => Fractional a where
    -- | Divides one fractional value by another.
    (/) : a -> a -> a

-- | Type constructors whose contents can be transformed.
class Functor f where
    -- | Applies a function inside a context.
    fmap : forall a b. (a -> b) -> f a -> f b

-- | Operator form of fmap.
(<$>) : forall f a b. Functor f => (a -> b) -> f a -> f b
(<$>) = fmap

-- | Functors that can lift values and apply wrapped functions.
class Functor f => Applicative f where
    -- | Lifts a plain value into a context.
    pure : forall a. a -> f a
    -- | Applies a wrapped function to a wrapped value.
    apply : forall a b. f (a -> b) -> f a -> f b

-- | Operator form of applicative function application.
(<*>) : forall f a b. Applicative f => f (a -> b) -> f a -> f b
(<*>) = apply

-- | Combines two contextual values with a binary function.
liftA2 : forall f a b c. Applicative f => (a -> b -> c) -> f a -> f b -> f c
liftA2 f fx fy = f <$> fx <*> fy

-- | Runs two contextual values and keeps the result of the second.
applyRight : forall f a b. Applicative f => f a -> f b -> f b
applyRight left right = liftA2 (fun x => fun y => y) left right

-- | Runs two contextual values and keeps the result of the first.
applyLeft : forall f a b. Applicative f => f a -> f b -> f a
applyLeft left right = liftA2 (fun x => fun y => x) left right

-- | Operator form of applyLeft.
(<*) : forall f a b. Applicative f => f a -> f b -> f a
(<*) = applyLeft

-- | Operator form of applyRight.
(*>) : forall f a b. Applicative f => f a -> f b -> f b
(*>) = applyRight

-- | Applicative contexts that can sequence dependent computations.
class Applicative m => Monad m where
    -- | Sequences a computation and passes its result to the continuation.
    bind : forall a b. m a -> (a -> m b) -> m b

-- | Operator form of bind.
(>>=) : forall m a b. Monad m => m a -> (a -> m b) -> m b
(>>=) = bind

-- | Runs two monadic computations and keeps the result of the second.
-- |
-- | This compiler-facing name is also the canonical do-notation contract.
thenM : forall m a b. Monad m => m a -> m b -> m b
thenM mx my = bind mx (fun x => my)

-- | Operator form of monadic sequencing.
(>>) : forall m a b. Monad m => m a -> m b -> m b
(>>) = thenM

-- | Values that can be rendered as text.
class Show a where
    -- | Renders a value as a string.
    show : a -> String

-- | Values that can be combined associatively.
class Semigroup a where
    -- | Combines two values.
    combine : a -> a -> a

-- | Operator form of associative combination.
(<>) : forall a. Semigroup a => a -> a -> a
(<>) = combine

-- | Semigroups that also provide an empty value.
class Semigroup a => Monoid a where
    -- | The identity value for combine.
    empty : a

-- | Combines all values in a list.
mconcat : forall a. Monoid a => [a] -> a
mconcat xs = foldr combine empty xs

-- | Containers whose finite contents can be reduced to one value.
-- |
-- | Solveig is strict, so `foldl` is always a strict left fold; there is no
-- | separate `foldl'`. Instances must preserve the container's logical order.
class Foldable t where
    -- | Folds from right to left without requiring a recursive user stack.
    foldr : forall a b. (a -> b -> b) -> b -> t a -> b
    -- | Strictly folds from left to right.
    foldl : forall a b. (b -> a -> b) -> b -> t a -> b

-- | Maps every element to a monoid and combines the results in logical order.
foldMap : forall t a m. (Foldable t, Monoid m) => (a -> m) -> t a -> m
foldMap transform values = foldr (fun value acc => combine (transform value) acc) empty values

-- | Functors whose elements can be visited through an Applicative effect.
-- |
-- | Traversal follows the same left-to-right order as `foldl`. For `IO`, that
-- | means effects are deterministic and sequential; traversal never adds
-- | concurrency implicitly.
class (Functor t, Foldable t) => Traversable t where
    -- | Applies an effectful transformation to every element.
    traverse : forall f a b. Applicative f => (a -> f b) -> t a -> f (t b)

-- | Turns a traversable container of effects into one effectful container.
sequenceA : forall t f a. (Traversable t, Applicative f) => t (f a) -> f (t a)
sequenceA values = traverse (fun value => value) values

-- | Values that can contribute to a deterministic machine-sized hash.
-- |
-- | Instances must obey: if `x == y`, then `hashWithSalt salt x ==
-- | hashWithSalt salt y`. Exact numeric hashes are not a serialization format
-- | and may change between Solveig releases. Float deliberately has no
-- | instance because its equality is not reflexive for NaN.
class Eq a => Hashable a where
    -- | Mixes a value into an existing salt.
    hashWithSalt : Int -> a -> Int

-- | Hashes a value from the standard initial salt.
hash : forall a. Hashable a => a -> Int
hash value = hashWithSalt 0 value

-- Package-internal deterministic mixing primitive. Int overflow is the
-- language's defined wrapping arithmetic and is intentional here.
mixHash : Int -> Int -> Int
mixHash salt value = salt * 16777619 + value
