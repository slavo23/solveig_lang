export (
    HashMap, empty, singleton, size, isEmpty, lookup, member, insert, delete,
    fromList, toList, toAscList, keys, values, union
)

import std.list(reverse)
import std.map

-- | An immutable hash map backed by a persistent hash-array-mapped trie.
-- |
-- | Dispersed hashes use at most thirteen 32-way bitmap nodes. Updates share
-- | untouched nodes and copy at most one compact child list per level. Equal
-- | hashes use a collision bucket with linear lookup in the bucket size.
data HashMap k v =
    HashEmpty
  | HashLeaf Int k v
  | HashCollision Int [(Int, k, v)]
  | HashBranch Int Int [HashMap k v]

branchWidth : Int
branchWidth = 32

maximumDepth : Int
maximumDepth = 13

-- | Returns an empty hash map without allocating trie nodes.
empty : forall k v. HashMap k v
empty = HashEmpty

-- | Returns a one-entry hash map.
singleton : forall k v. Hashable k => k -> v -> HashMap k v
singleton key value = HashLeaf (hash key) key value

-- | Returns the number of entries in constant time.
size : forall k v. HashMap k v -> Int
size HashEmpty = 0
size (HashLeaf _ _ _) = 1
size (HashCollision count _) = count
size (HashBranch count _ _) = count

-- | Reports whether the map is empty in constant time.
isEmpty : forall k v. HashMap k v -> Bool
isEmpty HashEmpty = True
isEmpty _ = False

-- | Finds a value by key in expected constant time.
-- |
-- | A full-hash collision takes linear time in that collision bucket.
lookup : forall k v. Hashable k => k -> HashMap k v -> Maybe v
lookup key map = lookupNode 0 (hash key) key map

lookupNode : forall k v. Eq k => Int -> Int -> k -> HashMap k v -> Maybe v
lookupNode _ _ _ HashEmpty = Nothing
lookupNode _ entryHash key (HashLeaf leafHash leafKey value) =
    if and (entryHash == leafHash) (key == leafKey) then Just value else Nothing
lookupNode _ entryHash key (HashCollision _ entries) =
    lookupCollision entryHash key entries
lookupNode depth entryHash key (HashBranch _ bitmap children) =
    let bit = bitFor (fragment (shiftHash depth entryHash)) in
    if hasBit bit bitmap then
        lookupNode (depth + 1) entryHash key (childAt (rankFor bit bitmap) children)
    else
        Nothing

lookupCollision : forall k v. Eq k => Int -> k -> [(Int, k, v)] -> Maybe v
lookupCollision _ _ [] = Nothing
lookupCollision entryHash key ((candidateHash, candidateKey, value) :: rest) =
    if and (entryHash == candidateHash) (key == candidateKey) then Just value
    else lookupCollision entryHash key rest

-- | Reports whether a key is present in expected constant time.
member : forall k v. Hashable k => k -> HashMap k v -> Bool
member key map =
    case lookup key map of
        Nothing => False
        | Just _ => True

-- | Inserts or replaces an entry in expected constant time.
-- |
-- | The returned map shares every untouched trie node. A full-hash collision
-- | takes linear time and allocation in the collision bucket size.
insert : forall k v. Hashable k => k -> v -> HashMap k v -> HashMap k v
insert key value map =
    case insertNode 0 (hash key) key value map of
        (_, inserted) => inserted

insertNode : forall k v. Eq k => Int -> Int -> k -> v -> HashMap k v -> (Bool, HashMap k v)
insertNode _ entryHash key value HashEmpty =
    (True, HashLeaf entryHash key value)
insertNode depth entryHash key value (HashLeaf leafHash leafKey leafValue) =
    if entryHash == leafHash then
        if key == leafKey then (False, HashLeaf entryHash key value)
        else
            ( True
            , HashCollision
                2
                [(leafHash, leafKey, leafValue), (entryHash, key, value)]
            )
    else
        ( True
        , mergeNodes
            depth
            leafHash
            (HashLeaf leafHash leafKey leafValue)
            entryHash
            (HashLeaf entryHash key value)
        )
insertNode depth entryHash key value (HashCollision count entries) =
    let representativeHash = collisionHash entries in
    if or (depth >= maximumDepth) (entryHash == representativeHash) then
        case insertCollision entryHash key value entries [] of
            (added, updated) =>
                (added, HashCollision (if added then count + 1 else count) updated)
    else
        ( True
        , mergeNodes
            depth
            representativeHash
            (HashCollision count entries)
            entryHash
            (HashLeaf entryHash key value)
        )
insertNode depth entryHash key value (HashBranch count bitmap children) =
    let bit = bitFor (fragment (shiftHash depth entryHash)) in
    let rank = rankFor bit bitmap in
    if hasBit bit bitmap then
        case insertNode (depth + 1) entryHash key value (childAt rank children) of
            (added, child) =>
                ( added
                , HashBranch
                    (if added then count + 1 else count)
                    bitmap
                    (replaceAt rank child children)
                )
    else
        ( True
        , HashBranch
            (count + 1)
            (bitmap + bit)
            (insertAt rank (HashLeaf entryHash key value) children)
        )

collisionHash : forall k v. [(Int, k, v)] -> Int
collisionHash ((entryHash, _, _) :: _) = entryHash
collisionHash [] = error "invalid empty hash map collision"

insertCollision : forall k v. Eq k => Int -> k -> v -> [(Int, k, v)] -> [(Int, k, v)] -> (Bool, [(Int, k, v)])
insertCollision entryHash key value [] reversed =
    (True, restoreEntries reversed [(entryHash, key, value)])
insertCollision entryHash key value ((candidateHash, candidateKey, candidateValue) :: rest) reversed =
    if and (entryHash == candidateHash) (key == candidateKey) then
        ( False
        , restoreEntries reversed ((entryHash, key, value) :: rest)
        )
    else
        insertCollision
            entryHash
            key
            value
            rest
            ((candidateHash, candidateKey, candidateValue) :: reversed)

restoreEntries : forall entry. [entry] -> [entry] -> [entry]
restoreEntries [] suffix = suffix
restoreEntries (entry :: rest) suffix = restoreEntries rest (entry :: suffix)

-- Bagwell, "Ideal Hash Trees" (2001): a bitmap selects a compact child list,
-- so persistent updates copy only the bounded search spine.
mergeNodes : forall k v. Int -> Int -> HashMap k v -> Int -> HashMap k v -> HashMap k v
mergeNodes depth leftHash left rightHash right =
    if depth >= maximumDepth then
        HashCollision
            (size left + size right)
            (hashedEntries left ++ hashedEntries right)
    else
        let leftBit = bitFor (fragment (shiftHash depth leftHash)) in
        let rightBit = bitFor (fragment (shiftHash depth rightHash)) in
        if leftBit == rightBit then
            HashBranch
                (size left + size right)
                leftBit
                [(mergeNodes (depth + 1) leftHash left rightHash right)]
        else if leftBit < rightBit then
            HashBranch (size left + size right) (leftBit + rightBit) [left, right]
        else
            HashBranch (size left + size right) (leftBit + rightBit) [right, left]

hashedEntries : forall k v. HashMap k v -> [(Int, k, v)]
hashedEntries (HashLeaf entryHash key value) = [(entryHash, key, value)]
hashedEntries (HashCollision _ entries) = entries
hashedEntries _ = error "invalid hash map collision merge"

-- | Removes a key in expected constant time when it is present.
-- |
-- | The original map remains valid and shares every untouched trie node.
delete : forall k v. Hashable k => k -> HashMap k v -> HashMap k v
delete key map =
    case deleteNode 0 (hash key) key map of
        (_, remaining) => remaining

deleteNode : forall k v. Eq k => Int -> Int -> k -> HashMap k v -> (Bool, HashMap k v)
deleteNode _ _ _ HashEmpty = (False, HashEmpty)
deleteNode _ entryHash key (HashLeaf leafHash leafKey leafValue) =
    if and (entryHash == leafHash) (key == leafKey) then (True, HashEmpty)
    else (False, HashLeaf leafHash leafKey leafValue)
deleteNode _ entryHash key (HashCollision count entries) =
    case deleteCollision entryHash key entries [] of
        (False, _) => (False, HashCollision count entries)
        | (True, remaining) => (True, collisionNode (count - 1) remaining)
deleteNode depth entryHash key (HashBranch count bitmap children) =
    let bit = bitFor (fragment (shiftHash depth entryHash)) in
    if not (hasBit bit bitmap) then (False, HashBranch count bitmap children)
    else
        let rank = rankFor bit bitmap in
        case deleteNode (depth + 1) entryHash key (childAt rank children) of
            (False, _) => (False, HashBranch count bitmap children)
            | (True, HashEmpty) =>
                ( True
                , compactBranch
                    (count - 1)
                    (bitmap - bit)
                    (removeAt rank children)
                )
            | (True, child) =>
                ( True
                , compactBranch
                    (count - 1)
                    bitmap
                    (replaceAt rank child children)
                )

deleteCollision : forall k v. Eq k => Int -> k -> [(Int, k, v)] -> [(Int, k, v)] -> (Bool, [(Int, k, v)])
deleteCollision _ _ [] reversed = (False, restoreEntries reversed [])
deleteCollision entryHash key ((candidateHash, candidateKey, candidateValue) :: rest) reversed =
    if and (entryHash == candidateHash) (key == candidateKey) then
        (True, restoreEntries reversed rest)
    else
        deleteCollision
            entryHash
            key
            rest
            ((candidateHash, candidateKey, candidateValue) :: reversed)

collisionNode : forall k v. Int -> [(Int, k, v)] -> HashMap k v
collisionNode 0 _ = HashEmpty
collisionNode 1 ((entryHash, key, value) :: _) = HashLeaf entryHash key value
collisionNode count entries = HashCollision count entries

compactBranch : forall k v. Int -> Int -> [HashMap k v] -> HashMap k v
compactBranch 0 _ _ = HashEmpty
compactBranch _ _ (HashLeaf entryHash key value :: []) = HashLeaf entryHash key value
compactBranch _ _ (HashCollision count entries :: []) = HashCollision count entries
compactBranch count bitmap children = HashBranch count bitmap children

-- | Builds a map from key-value tuples, keeping the last value for a key.
-- |
-- | Construction is linear in the number of entries at expected constant
-- | insertion cost and preserves every intermediate map persistently.
fromList : forall k v. Hashable k => [(k, v)] -> HashMap k v
fromList entries = insertEntries entries empty

insertEntries : forall k v. Hashable k => [(k, v)] -> HashMap k v -> HashMap k v
insertEntries [] map = map
insertEntries ((key, value) :: rest) map =
    insertEntries rest (insert key value map)

-- | Returns entries in deterministic trie order in linear time.
-- |
-- | Branches are visited by ascending five-bit hash fragment. Keys sharing a
-- | complete hash retain collision-bucket insertion order. The result allocates
-- | one list cell per entry.
toList : forall k v. HashMap k v -> [(k, v)]
toList map = reverse (collectNodes [map] [])

collectNodes : forall k v. [HashMap k v] -> [(k, v)] -> [(k, v)]
collectNodes [] reversed = reversed
collectNodes (HashEmpty :: pending) reversed = collectNodes pending reversed
collectNodes (HashLeaf _ key value :: pending) reversed =
    collectNodes pending ((key, value) :: reversed)
collectNodes (HashCollision _ entries :: pending) reversed =
    collectCollision entries pending reversed
collectNodes (HashBranch _ _ children :: pending) reversed =
    collectNodes (children ++ pending) reversed

collectCollision : forall k v. [(Int, k, v)] -> [HashMap k v] -> [(k, v)] -> [(k, v)]
collectCollision [] pending reversed = collectNodes pending reversed
collectCollision ((_, key, value) :: rest) pending reversed =
    collectCollision rest pending ((key, value) :: reversed)

-- | Returns entries in ascending key order in O(n log n) time.
-- |
-- | This conversion is the canonical boundary for user-visible ordering and
-- | allocates an ordered map plus the returned list.
toAscList : forall k v. Ord k => HashMap k v -> [(k, v)]
toAscList map = std.map.toAscList (std.map.fromList (toList map))

-- | Returns keys in deterministic trie order in linear time.
keys : forall k v. HashMap k v -> [k]
keys = map (fun (key, _) => key) . toList

-- | Returns values in deterministic trie order in linear time.
values : forall k v. HashMap k v -> [v]
values = map (fun (_, value) => value) . toList

-- | Returns the left-biased union of two maps.
-- |
-- | Runs in expected O(n) time for the left map and shares structure from the
-- | right map until a conflicting search spine is replaced.
union : forall k v. Hashable k => HashMap k v -> HashMap k v -> HashMap k v
union left right = insertEntries (toList left) right

fragment : Int -> Int
fragment value =
    let remainder = mod value branchWidth in
    if remainder < 0 then remainder + branchWidth else remainder

shiftHash : Int -> Int -> Int
shiftHash 0 value = value
shiftHash depth value = shiftHash (depth - 1) (div value branchWidth)

bitFor : Int -> Int
bitFor 0 = 1
bitFor index = 2 * bitFor (index - 1)

hasBit : Int -> Int -> Bool
hasBit bit bitmap = mod (div bitmap bit) 2 == 1

rankFor : Int -> Int -> Int
rankFor bit bitmap = countBits (mod bitmap bit) 0

countBits : Int -> Int -> Int
countBits 0 count = count
countBits value count = countBits (div value 2) (count + mod value 2)

childAt : forall a. Int -> [a] -> a
childAt 0 (value :: _) = value
childAt index (_ :: rest) = childAt (index - 1) rest
childAt _ [] = error "invalid hash map child rank"

insertAt : forall a. Int -> a -> [a] -> [a]
insertAt 0 value values = value :: values
insertAt index value (current :: rest) = current :: insertAt (index - 1) value rest
insertAt _ value [] = [value]

replaceAt : forall a. Int -> a -> [a] -> [a]
replaceAt 0 value (_ :: rest) = value :: rest
replaceAt index value (current :: rest) = current :: replaceAt (index - 1) value rest
replaceAt _ _ [] = error "invalid hash map child replacement"

removeAt : forall a. Int -> [a] -> [a]
removeAt 0 (_ :: rest) = rest
removeAt index (current :: rest) = current :: removeAt (index - 1) rest
removeAt _ [] = error "invalid hash map child removal"

mapNodeValues : forall k a b. (a -> b) -> HashMap k a -> HashMap k b
mapNodeValues _ HashEmpty = HashEmpty
mapNodeValues transform (HashLeaf entryHash key value) =
    HashLeaf entryHash key (transform value)
mapNodeValues transform (HashCollision count entries) =
    HashCollision count (mapCollisionValues transform entries)
mapNodeValues transform (HashBranch count bitmap children) =
    HashBranch count bitmap (map (mapNodeValues transform) children)

mapCollisionValues : forall k a b. (a -> b) -> [(Int, k, a)] -> [(Int, k, b)]
mapCollisionValues transform entries = map (mapCollisionValue transform) entries

mapCollisionValue : forall k a b. (a -> b) -> (Int, k, a) -> (Int, k, b)
mapCollisionValue transform (entryHash, key, value) =
    (entryHash, key, transform value)

equalEntries : forall k v. (Hashable k, Eq v) => HashMap k v -> [(k, v)] -> Bool
equalEntries _ [] = True
equalEntries map ((key, value) :: rest) =
    case lookup key map of
        Nothing => False
        | Just existing =>
            if existing == value then equalEntries map rest else False

hashEntries : forall k v. (Hashable k, Hashable v) => Int -> [(k, v)] -> Int
hashEntries total [] = total
hashEntries total ((key, value) :: rest) =
    let entryHash = hashWithSalt (hashWithSalt 0 key) value in
    hashEntries (total + entryHash) rest

traverseCollisionEntry : forall f k a b. Applicative f => (a -> f b) -> (Int, k, a) -> f (Int, k, b)
traverseCollisionEntry transform (entryHash, key, value) =
    fmap (fun transformed => (entryHash, key, transformed)) (transform value)

instance Functor (HashMap k) where
    fmap = mapNodeValues

instance Foldable (HashMap k) where
    foldr combine initial map =
        foldr (fun (_, value) acc => combine value acc) initial (toList map)
    foldl combine initial map =
        foldl (fun acc (_, value) => combine acc value) initial (toList map)

instance Traversable (HashMap k) where
    traverse transform HashEmpty = pure HashEmpty
    traverse transform (HashLeaf entryHash key value) =
        fmap (HashLeaf entryHash key) (transform value)
    traverse transform (HashCollision count entries) =
        fmap
            (HashCollision count)
            (traverse (traverseCollisionEntry transform) entries)
    traverse transform (HashBranch count bitmap children) =
        fmap
            (HashBranch count bitmap)
            (traverse (traverse transform) children)

instance (Hashable k, Eq v) => Eq (HashMap k v) where
    (==) left right =
        and (size left == size right) (equalEntries left (toList right))

instance (Ord k, Show k, Show v) => Show (HashMap k v) where
    show map = "fromList " <> show (toAscList map)

instance (Hashable k, Hashable v) => Hashable (HashMap k v) where
    hashWithSalt salt map =
        hashWithSalt
            (hashWithSalt salt (size map))
            (hashEntries 0 (toList map))
