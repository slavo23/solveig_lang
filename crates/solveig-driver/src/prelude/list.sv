import prelude.builtins
import prelude.prim

-- | A finite sequence of values.
data List a =
    -- | Adds one value to the front of a list.
    Cons a (List a)
  | -- | The empty list.
    Nil

-- | Applies a function to every element of a list.
map : forall a b. (a -> b) -> [a] -> [b]
map f [] = []
map f (y :: ys) = f y :: map f ys

-- | Keeps the elements that satisfy a predicate.
filter : forall a. (a -> Bool) -> [a] -> [a]
filter f [] = []
filter f (y :: ys) = if f y then y :: filter f ys else filter f ys

-- | Folds a list from right to left.
foldr : forall a b. (a -> b -> b) -> b -> [a] -> b
foldr f z [] = z
foldr f z (y :: ys) = f y (foldr f z ys)

-- | Folds a list from left to right.
foldl : forall a b. (b -> a -> b) -> b -> [a] -> b
foldl f acc [] = acc
foldl f acc (y :: ys) = foldl f (f acc y) ys

-- | Flattens a list of lists into one list.
concat : forall a. [[a]] -> [a]
concat xss = foldr append [] xss

-- | Appends the second list after the first.
append : forall a. [a] -> [a] -> [a]
append [] ys = ys
append (z :: zs) ys = z :: append zs ys

infixr 5 (++)

(++) : forall a. [a] -> [a] -> [a]
(++) = append

-- | Reverses the order of the elements in a list.
reverse : forall a. [a] -> [a]
reverse xs =
    reverseGo xs [];

-- | Reverses a list into an accumulator.
reverseGo : forall a. [a] -> [a] -> [a]
reverseGo [] acc = acc;
reverseGo (y :: ys) acc = reverseGo ys (y :: acc)

-- | Builds an ascending integer range.
range : Int -> Int -> Bool -> [Int]
range start end inclusive =
    if inclusive then
        rangeDown end start []
    else
        rangeDown (intSub end 1) start []

-- | Builds an ascending range by counting down into an accumulator.
rangeDown : Int -> Int -> [Int] -> [Int]
rangeDown current start acc =
    if intLt current start then acc
    else rangeDown (intSub current 1) start (current :: acc)
