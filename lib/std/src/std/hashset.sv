export (
    HashSet,
    empty,
    singleton,
    size,
    isEmpty,
    member,
    insert,
    delete,
    fromList,
    toList,
    toAscList,
    union,
    intersection,
    difference
)

import std.hashmap

-- | An immutable hash set backed by a persistent hash-array-mapped trie.
-- |
-- | The set inherits `HashMap`'s bounded bitmap-trie path and linear
-- | full-hash collision behavior while storing no separate element payload.
data HashSet a = MkHashSet (std.hashmap.HashMap a ())

-- | Returns an empty hash set without allocating trie nodes.
empty : forall a. HashSet a
empty = MkHashSet std.hashmap.empty

-- | Returns a one-element hash set.
singleton : forall a. Hashable a => a -> HashSet a
singleton value = MkHashSet (std.hashmap.singleton value ())

-- | Returns the number of elements in constant time.
size : forall a. HashSet a -> Int
size (MkHashSet map) = std.hashmap.size map

-- | Reports whether the set is empty in constant time.
isEmpty : forall a. HashSet a -> Bool
isEmpty (MkHashSet map) = std.hashmap.isEmpty map

-- | Reports membership in expected constant time.
-- |
-- | A full-hash collision takes linear time in that collision bucket.
member : forall a. Hashable a => a -> HashSet a -> Bool
member value (MkHashSet map) = std.hashmap.member value map

-- | Inserts an element in expected constant time, sharing untouched nodes.
insert : forall a. Hashable a => a -> HashSet a -> HashSet a
insert value (MkHashSet map) = MkHashSet (std.hashmap.insert value () map)

-- | Removes an element in expected constant time when it is present.
delete : forall a. Hashable a => a -> HashSet a -> HashSet a
delete value (MkHashSet map) = MkHashSet (std.hashmap.delete value map)

-- | Builds a set, discarding duplicate elements.
fromList : forall a. Hashable a => [a] -> HashSet a
fromList values = insertValues values empty

insertValues : forall a. Hashable a => [a] -> HashSet a -> HashSet a
insertValues [] set = set
insertValues (value :: rest) set = insertValues rest (insert value set)

-- | Returns elements in deterministic trie order in linear time.
-- |
-- | Complete-hash collisions retain insertion order. The result allocates one
-- | list cell per distinct element.
toList : forall a. HashSet a -> [a]
toList (MkHashSet map) = std.hashmap.keys map

-- | Returns elements in ascending order in O(n log n) time.
-- |
-- | This is the canonical conversion for user-visible ordering.
toAscList : forall a. Ord a => HashSet a -> [a]
toAscList (MkHashSet dictionary) = map (fun (value, _) => value) (std.hashmap.toAscList dictionary)

-- | Returns the union of two sets in expected linear time in the left set.
union : forall a. Hashable a => HashSet a -> HashSet a -> HashSet a
union left right = insertValues (toList left) right

-- | Returns elements present in both sets.
-- |
-- | Runs in expected linear time in the first set and allocates the result.
intersection : forall a. Hashable a => HashSet a -> HashSet a -> HashSet a
intersection left right = intersectionValues right (toList left) empty

intersectionValues : forall a. Hashable a => HashSet a -> [a] -> HashSet a -> HashSet a
intersectionValues _ [] result = result
intersectionValues source (value :: rest) result =
    if member value source
        then intersectionValues source rest (insert value result)
        else intersectionValues source rest result

-- | Returns elements present in the first set but not the second.
-- |
-- | Runs in expected linear time in the first set and allocates the result.
difference : forall a. Hashable a => HashSet a -> HashSet a -> HashSet a
difference left right = differenceValues right (toList left) empty

differenceValues : forall a. Hashable a => HashSet a -> [a] -> HashSet a -> HashSet a
differenceValues _ [] result = result
differenceValues removed (value :: rest) result =
    if member value removed
        then differenceValues removed rest result
        else differenceValues removed rest (insert value result)

equalElements : forall a. Hashable a => HashSet a -> [a] -> Bool
equalElements _ [] = True
equalElements set (value :: rest) = if member value set then equalElements set rest else False

hashElements : forall a. Hashable a => Int -> [a] -> Int
hashElements total [] = total
hashElements total (value :: rest) = hashElements (total + hash value) rest

instance Foldable HashSet where
    foldr combine initial set = foldr combine initial (toList set)
    foldl combine initial set = foldl combine initial (toList set)

instance Hashable a => Eq (HashSet a) where
    (==) left right = and (size left == size right) (equalElements left (toList right))

instance (Ord a, Show a) => Show (HashSet a) where
    show set = "fromList " <> show (toAscList set)

instance Hashable a => Hashable (HashSet a) where
    hashWithSalt salt set =
        hashWithSalt (hashWithSalt salt (size set)) (hashElements 0 (toList set))

instance Hashable a => Semigroup (HashSet a) where
    combine left right = union left right

instance Hashable a => Monoid (HashSet a) where
    empty = MkHashSet std.hashmap.empty
