export (List(..), map, filter, concat, append, (++))

import prelude.builtins(*)
import prelude.prim(*)

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

-- Package-internal implementation of the strict Foldable List fold.
foldlList : forall a b. (b -> a -> b) -> b -> [a] -> b
foldlList combine acc [] = acc
foldlList combine acc (x :: xs) = foldlList combine (combine acc x) xs

-- Package-internal stack-safe implementation of the Foldable List right fold.
foldrList : forall a b. (a -> b -> b) -> b -> [a] -> b
foldrList combine initial values =
    foldlList (fun acc value => combine value acc) initial (reverseList values)

-- Package-internal stack-safe list reversal shared by Prelude instances.
reverseList : forall a. [a] -> [a]
reverseList values = reverseListInto values []

reverseListInto : forall a. [a] -> [a] -> [a]
reverseListInto [] reversed = reversed
reverseListInto (value :: rest) reversed = reverseListInto rest (value :: reversed)

-- | Flattens a list of lists into one list.
concat : forall a. [[a]] -> [a]
concat lists = foldrList append [] lists

-- | Appends the second list after the first.
append : forall a. [a] -> [a] -> [a]
append [] ys = ys
append (x :: xs) ys = x :: append xs ys

infixr 5 (++)

-- | Appends the second list after the first.
(++) : forall a. [a] -> [a] -> [a]
(++) = append
