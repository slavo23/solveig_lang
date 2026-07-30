import prelude.builtins;
import prelude.prim;

-- | A finite sequence of values.
data List a =
    -- | Adds one value to the front of a list.
    Cons a (List a)
  | -- | The empty list.
    Nil;

-- | Applies a function to every element of a list.
map : forall a b. (a -> b) -> [a] -> [b];
map f xs =
    case xs of
        [] => []
        | y :: ys => f y :: map f ys;

-- | Keeps the elements that satisfy a predicate.
filter : forall a. (a -> Bool) -> [a] -> [a];
filter f xs =
    case xs of
        [] => []
        | y :: ys => if f y then y :: filter f ys else filter f ys;

-- | Folds a list from right to left.
foldr : forall a b. (a -> b -> b) -> b -> [a] -> b;
foldr f z xs =
    case xs of
        [] => z
        | y :: ys => f y (foldr f z ys);

-- | Folds a list from left to right.
foldl : forall a b. (b -> a -> b) -> b -> [a] -> b;
foldl f acc xs =
    case xs of
        [] => acc
        | y :: ys => foldl f (f acc y) ys;

-- | Flattens a list of lists into one list.
concat : forall a. [[a]] -> [a]
concat xss = foldr append [] xss

-- | Appends the second list after the first.
append : forall a. [a] -> [a] -> [a];
append xs ys =
    case xs of
        [] => ys
        | z :: zs => z :: append zs ys;

-- | Reverses the order of the elements in a list.
reverse : forall a. [a] -> [a];
reverse xs =
    reverseGo xs [];

-- | Reverses a list into an accumulator.
reverseGo : forall a. [a] -> [a] -> [a];
reverseGo xs acc =
    case xs of
        [] => acc
        | y :: ys => reverseGo ys (y :: acc);

-- | Builds an ascending integer range.
range : Int -> Int -> Bool -> [Int];
range start end inclusive =
    if inclusive then
        rangeDown end start []
    else
        rangeDown (intSub end 1) start [];

-- | Builds an ascending range by counting down into an accumulator.
rangeDown : Int -> Int -> [Int] -> [Int];
rangeDown current start acc =
    if intLt current start then acc
    else rangeDown (intSub current 1) start (current :: acc);
