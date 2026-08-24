export (
    Set, empty, singleton, size, member, insert, delete, fromList, toAscList, union,
    intersection, difference
)

import std.map

-- | An immutable ordered set backed by `std.map`.
data Set a = MkSet (std.map.Map a ())

-- | Returns an empty set.
empty : forall a. Set a
empty = MkSet std.map.empty

-- | Returns a one-element set.
singleton : forall a. a -> Set a
singleton value = MkSet (std.map.singleton value ())

-- | Returns the number of elements in constant time.
size : forall a. Set a -> Int
size (MkSet map) = std.map.size map

-- | Reports whether an element is present.
member : forall a. Ord a => a -> Set a -> Bool
member value (MkSet map) = std.map.member value map

-- | Inserts an element in logarithmic time.
insert : forall a. Ord a => a -> Set a -> Set a
insert value (MkSet map) = MkSet (std.map.insert value () map)

-- | Removes an element in logarithmic time when it is present.
delete : forall a. Ord a => a -> Set a -> Set a
delete value (MkSet map) = MkSet (std.map.delete value map)

-- | Builds a set, discarding duplicate elements.
fromList : forall a. Ord a => [a] -> Set a
fromList values = fromListInto empty values

fromListInto : forall a. Ord a => Set a -> [a] -> Set a
fromListInto set [] = set
fromListInto set (value :: rest) = fromListInto (insert value set) rest

-- | Returns elements in ascending order.
toAscList : forall a. Set a -> [a]
toAscList (MkSet map) = std.map.keys map

-- | Returns the union of two sets.
union : forall a. Ord a => Set a -> Set a -> Set a
union left right = unionValues left (toAscList right)

unionValues : forall a. Ord a => Set a -> [a] -> Set a
unionValues set [] = set
unionValues set (value :: rest) = unionValues (insert value set) rest

-- | Returns the elements present in both sets.
intersection : forall a. Ord a => Set a -> Set a -> Set a
intersection left right = intersectionValues right (toAscList left) empty

intersectionValues : forall a. Ord a => Set a -> [a] -> Set a -> Set a
intersectionValues source [] result = result
intersectionValues source (value :: rest) result =
    if member value source then intersectionValues source rest (insert value result)
    else intersectionValues source rest result

-- | Returns the elements present in the first set but not the second.
difference : forall a. Ord a => Set a -> Set a -> Set a
difference left right = differenceValues right (toAscList left) empty

differenceValues : forall a. Ord a => Set a -> [a] -> Set a -> Set a
differenceValues removed [] result = result
differenceValues removed (value :: rest) result =
    if member value removed then differenceValues removed rest result
    else differenceValues removed rest (insert value result)

instance Eq a => Eq (Set a) where
    (==) left right = toAscList left == toAscList right

instance Ord a => Semigroup (Set a) where
    combine left right = union left right

instance Ord a => Monoid (Set a) where
    empty = MkSet std.map.empty
