export (List(..), map, filter, concat, append, (++))

import prelude.builtins(*)
import prelude.prim(*)

-- | A finite sequence of values.
data List a =
    -- | Adds one value to the front of a list.
    Cons a (List a)
    | -- | The empty list.
    Nil

-- | Applies a function to every element from left to right.
-- |
-- | Runs in linear time and uses a linear temporary list so user-sized finite
-- | inputs do not consume the call stack.
map : forall a b. (a -> b) -> [a] -> [b]
map transform values = reverseList (mapInto transform values [])

mapInto : forall a b. (a -> b) -> [a] -> [b] -> [b]
mapInto transform [] reversed = reversed
mapInto transform (value :: rest) reversed = mapInto transform rest (transform value :: reversed)

-- | Keeps the elements that satisfy a predicate, testing from left to right.
-- |
-- | Runs in linear time and uses a linear temporary list so user-sized finite
-- | inputs do not consume the call stack.
filter : forall a. (a -> Bool) -> [a] -> [a]
filter predicate values = reverseList (filterInto predicate values [])

filterInto : forall a. (a -> Bool) -> [a] -> [a] -> [a]
filterInto predicate [] reversed = reversed
filterInto predicate (value :: rest) reversed =
    if predicate value
        then filterInto predicate rest (value :: reversed)
        else filterInto predicate rest reversed

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

-- | Flattens a finite list of finite lists in linear time.
-- |
-- | Input order is preserved and traversal does not consume the call stack.
concat : forall a. [[a]] -> [a]
concat lists = foldrList append [] lists

-- | Appends the second list after the first in time linear in the first list.
-- |
-- | The second list is retained. A linear temporary copy of the first list
-- | keeps the operation stack-safe.
append : forall a. [a] -> [a] -> [a]
append xs ys = foldlList (fun tail value => value :: tail) ys (reverseList xs)

infixr 5 (++)

-- | Appends the second list after the first.
(++) : forall a. [a] -> [a] -> [a]
(++) = append
