export (List(..), map, filter, foldr, foldl, concat, append, (++))

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
map transform [] = []
map transform (x :: xs) = transform x :: map transform xs

-- | Keeps the elements that satisfy a predicate.
filter : forall a. (a -> Bool) -> [a] -> [a]
filter predicate [] = []
filter predicate (x :: xs) =
    if predicate x then x :: filter predicate xs else filter predicate xs

-- | Folds a list from right to left.
foldr : forall a b. (a -> b -> b) -> b -> [a] -> b
foldr combine initial [] = initial
foldr combine initial (x :: xs) = combine x (foldr combine initial xs)

-- | Folds a list from left to right.
foldl : forall a b. (b -> a -> b) -> b -> [a] -> b
foldl combine acc [] = acc
foldl combine acc (x :: xs) = foldl combine (combine acc x) xs

-- | Flattens a list of lists into one list.
concat : forall a. [[a]] -> [a]
concat lists = foldr append [] lists

-- | Appends the second list after the first.
append : forall a. [a] -> [a] -> [a]
append [] ys = ys
append (x :: xs) ys = x :: append xs ys

infixr 5 (++)

-- | Appends the second list after the first.
(++) : forall a. [a] -> [a] -> [a]
(++) = append
