export (
    Entry(..), Map, empty, singleton, size, lookup, member, insert, delete, fromList,
    toAscList, keys, values
)

-- | A key-value pair used by ordered map traversal and construction.
data Entry k v = Entry k v

-- | An immutable ordered map backed by a persistent AVL tree.
data Map k v =
    MapEmpty
  | MapNode Int Int k v (Map k v) (Map k v)

data RemoveMinimum k v =
    NoMinimum
  | RemovedMinimum k v (Map k v)

-- | Returns an empty map.
empty : forall k v. Map k v
empty = MapEmpty

-- | Returns a map containing one key-value pair.
singleton : forall k v. k -> v -> Map k v
singleton key value = MapNode 1 1 key value MapEmpty MapEmpty

mapHeight : forall k v. Map k v -> Int
mapHeight MapEmpty = 0
mapHeight (MapNode height _ _ _ _ _) = height

-- | Returns the number of entries in constant time.
size : forall k v. Map k v -> Int
size MapEmpty = 0
size (MapNode _ count _ _ _ _) = count

makeNode : forall k v. k -> v -> Map k v -> Map k v -> Map k v
makeNode key value left right =
    MapNode
        (1 + maxOrd (mapHeight left) (mapHeight right))
        (1 + size left + size right)
        key
        value
        left
        right

rotateLeft : forall k v. Map k v -> Map k v
rotateLeft (MapNode _ _ key value left right) =
    case right of
        MapNode _ _ rightKey rightValue rightLeft rightRight =>
            makeNode rightKey rightValue (makeNode key value left rightLeft) rightRight
        | MapEmpty => makeNode key value left right
rotateLeft MapEmpty = MapEmpty

rotateRight : forall k v. Map k v -> Map k v
rotateRight (MapNode _ _ key value left right) =
    case left of
        MapNode _ _ leftKey leftValue leftLeft leftRight =>
            makeNode leftKey leftValue leftLeft (makeNode key value leftRight right)
        | MapEmpty => makeNode key value left right
rotateRight MapEmpty = MapEmpty

-- Adelson-Velsky and Landis (1962): insertion and deletion repair only the
-- modified search spine, retaining logarithmic map operations.
balance : forall k v. k -> v -> Map k v -> Map k v -> Map k v
balance key value left right =
    let leftHeight = mapHeight left in
    let rightHeight = mapHeight right in
    if leftHeight > rightHeight + 1 then
        case left of
            MapNode _ _ _ _ leftLeft leftRight =>
                if mapHeight leftLeft >= mapHeight leftRight then
                    rotateRight (makeNode key value left right)
                else
                    rotateRight (makeNode key value (rotateLeft left) right)
            | MapEmpty => makeNode key value left right
    else if rightHeight > leftHeight + 1 then
        case right of
            MapNode _ _ _ _ rightLeft rightRight =>
                if mapHeight rightRight >= mapHeight rightLeft then
                    rotateLeft (makeNode key value left right)
                else
                    rotateLeft (makeNode key value left (rotateRight right))
            | MapEmpty => makeNode key value left right
    else
        makeNode key value left right

-- | Finds a value by key in logarithmic time.
lookup : forall k v. Ord k => k -> Map k v -> Maybe v
lookup _ MapEmpty = Nothing
lookup key (MapNode _ _ nodeKey nodeValue left right) =
    case compare key nodeKey of
        LT => lookup key left
        | EQ => Just nodeValue
        | GT => lookup key right

-- | Reports whether a key is present.
member : forall k v. Ord k => k -> Map k v -> Bool
member key tree =
    case lookup key tree of
        Nothing => False
        | Just _ => True

-- | Inserts or replaces a key-value pair in logarithmic time.
insert : forall k v. Ord k => k -> v -> Map k v -> Map k v
insert key value MapEmpty = singleton key value
insert key value (MapNode _ _ nodeKey nodeValue left right) =
    case compare key nodeKey of
        LT => balance nodeKey nodeValue (insert key value left) right
        | EQ => makeNode key value left right
        | GT => balance nodeKey nodeValue left (insert key value right)

removeMinimum : forall k v. Map k v -> RemoveMinimum k v
removeMinimum MapEmpty = NoMinimum
removeMinimum (MapNode _ _ key value MapEmpty right) = RemovedMinimum key value right
removeMinimum (MapNode _ _ key value left right) =
    case removeMinimum left of
        NoMinimum => RemovedMinimum key value right
        | RemovedMinimum minimumKey minimumValue newLeft =>
            RemovedMinimum minimumKey minimumValue (balance key value newLeft right)

join : forall k v. Map k v -> Map k v -> Map k v
join MapEmpty right = right
join left MapEmpty = left
join left right =
    case removeMinimum right of
        NoMinimum => left
        | RemovedMinimum key value rightRest => balance key value left rightRest

-- | Removes a key in logarithmic time when it is present.
delete : forall k v. Ord k => k -> Map k v -> Map k v
delete _ MapEmpty = MapEmpty
delete key (MapNode _ _ nodeKey nodeValue left right) =
    case compare key nodeKey of
        LT => balance nodeKey nodeValue (delete key left) right
        | EQ => join left right
        | GT => balance nodeKey nodeValue left (delete key right)

-- | Builds a map, keeping the last value for each repeated key.
fromList : forall k v. Ord k => [Entry k v] -> Map k v
fromList entries = fromListInto empty entries

fromListInto : forall k v. Ord k => Map k v -> [Entry k v] -> Map k v
fromListInto map [] = map
fromListInto map (Entry key value :: rest) =
    fromListInto (insert key value map) rest

-- | Returns entries in ascending key order.
toAscList : forall k v. Map k v -> [Entry k v]
toAscList map = toAscListInto map []

toAscListInto : forall k v. Map k v -> [Entry k v] -> [Entry k v]
toAscListInto MapEmpty acc = acc
toAscListInto (MapNode _ _ key value left right) acc =
    toAscListInto left (Entry key value :: toAscListInto right acc)

-- | Returns keys in ascending order.
keys : forall k v. Map k v -> [k]
keys map = keysInto map []

keysInto : forall k v. Map k v -> [k] -> [k]
keysInto MapEmpty acc = acc
keysInto (MapNode _ _ key _ left right) acc =
    keysInto left (key :: keysInto right acc)

-- | Returns values in ascending key order.
values : forall k v. Map k v -> [v]
values map = valuesInto map []

valuesInto : forall k v. Map k v -> [v] -> [v]
valuesInto MapEmpty acc = acc
valuesInto (MapNode _ _ _ value left right) acc =
    valuesInto left (value :: valuesInto right acc)

instance Functor (Map k) where
    fmap transform MapEmpty = MapEmpty
    fmap transform (MapNode height count key value left right) =
        MapNode height count key (transform value) (fmap transform left) (fmap transform right)
