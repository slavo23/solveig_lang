data Ord3 = Lt | Eq | Gt

class OrdKey k where
    cmp : k -> k -> Ord3

data Map k v = Empty | Node Int k v (Map k v) (Map k v)

data Entry k v = Entry k v

data PopMin k v = NoPopMin | PoppedMin k v (Map k v)

max2 : Int -> Int -> Int
max2 a b = if a < b then b else a

empty : forall k v. Map k v
empty = Empty

singleton : forall k v. k -> v -> Map k v
singleton key value = Node 1 key value Empty Empty

height : forall k v. Map k v -> Int
height tree =
    case tree of
        Empty => 0
        | Node h _ _ _ _ => h

mkNode : forall k v. k -> v -> Map k v -> Map k v -> Map k v
mkNode key value left right = Node (1 + max2 (height left) (height right)) key value left right

rotateLeft : forall k v. Map k v -> Map k v
rotateLeft tree =
    case tree of
        Empty => Empty
        | Node _ key value left right =>
            case right of
                Empty => tree
                | Node _ rKey rValue rLeft rRight =>
                    mkNode rKey rValue (mkNode key value left rLeft) rRight

rotateRight : forall k v. Map k v -> Map k v
rotateRight tree =
    case tree of
        Empty => Empty
        | Node _ key value left right =>
            case left of
                Empty => tree
                | Node _ lKey lValue lLeft lRight =>
                    mkNode lKey lValue lLeft (mkNode key value lRight right)

rebalance : forall k v. k -> v -> Map k v -> Map k v -> Map k v
rebalance key value left right =
    let hl =
        height left
    in let hr =
        height right
    in if hl > hr + 1
        then
            case left of
                Empty => mkNode key value left right
                | Node _ _ _ ll lr =>
                    if (height ll) >= (height lr)
                        then rotateRight (mkNode key value left right)
                        else rotateRight (mkNode key value (rotateLeft left) right)
        else if hr > hl + 1
                then
                    case right of
                        Empty => mkNode key value left right
                        | Node _ _ _ rl rr =>
                            if (height rr) >= (height rl)
                                then rotateLeft (mkNode key value left right)
                                else rotateLeft (mkNode key value left (rotateRight right))
                else mkNode key value left right

lookup : forall k v. OrdKey k => k -> Map k v -> Maybe v
lookup key tree =
    case tree of
        Empty => Nothing
        | Node _ nodeKey nodeValue left right =>
            case cmp key nodeKey of
                Lt => lookup key left
                | Eq => Just nodeValue
                | Gt => lookup key right

member : forall k v. OrdKey k => k -> Map k v -> Bool
member key tree =
    case lookup key tree of
        Nothing => False
        | Just _ => True

insert : forall k v. OrdKey k => k -> v -> Map k v -> Map k v
insert key value tree =
    case tree of
        Empty => singleton key value
        | Node _ nodeKey nodeValue left right =>
            case cmp key nodeKey of
                Lt => rebalance nodeKey nodeValue (insert key value left) right
                | Eq => mkNode key value left right
                | Gt => rebalance nodeKey nodeValue left (insert key value right)

popMin : forall k v. Map k v -> PopMin k v
popMin tree =
    case tree of
        Empty => NoPopMin
        | Node _ key value left right =>
            case left of
                Empty => PoppedMin key value right
                | _ =>
                    case popMin left of
                        NoPopMin => PoppedMin key value right
                        | PoppedMin minKey minValue newLeft =>
                            PoppedMin minKey minValue (rebalance key value newLeft right)

joinMaps : forall k v. Map k v -> Map k v -> Map k v
joinMaps left right =
    case left of
        Empty => right
        | _ =>
            case right of
                Empty => left
                | _ =>
                    case popMin right of
                        NoPopMin => left
                        | PoppedMin minKey minValue rightRest =>
                            rebalance minKey minValue left rightRest

delete : forall k v. OrdKey k => k -> Map k v -> Map k v
delete key tree =
    case tree of
        Empty => Empty
        | Node _ nodeKey nodeValue left right =>
            case cmp key nodeKey of
                Lt => rebalance nodeKey nodeValue (delete key left) right
                | Eq => joinMaps left right
                | Gt => rebalance nodeKey nodeValue left (delete key right)

size : forall k v. Map k v -> Int
size tree =
    case tree of
        Empty => 0
        | Node _ _ _ left right => 1 + size left + size right

toAscListGo : forall k v. Map k v -> [Entry k v] -> [Entry k v]
toAscListGo tree acc =
    case tree of
        Empty => acc
        | Node _ key value left right => toAscListGo left (Entry key value :: toAscListGo right acc)

toAscList : forall k v. Map k v -> [Entry k v]
toAscList tree = toAscListGo tree []

fromListGo : forall k v. OrdKey k => Map k v -> [Entry k v] -> Map k v
fromListGo acc xs =
    case xs of
        [] => acc
        | Entry key value :: rest => fromListGo (insert key value acc) rest

fromList : forall k v. OrdKey k => [Entry k v] -> Map k v
fromList xs = fromListGo Empty xs

keysGo : forall k v. Map k v -> [k] -> [k]
keysGo tree acc =
    case tree of
        Empty => acc
        | Node _ key _ left right => keysGo left (key :: keysGo right acc)

keys : forall k v. Map k v -> [k]
keys tree = keysGo tree []

valuesGo : forall k v. Map k v -> [v] -> [v]
valuesGo tree acc =
    case tree of
        Empty => acc
        | Node _ _ value left right => valuesGo left (value :: valuesGo right acc)

values : forall k v. Map k v -> [v]
values tree = valuesGo tree []

instance OrdKey Int where
    cmp x y = if x < y then Lt else if x > y then Gt else Eq

exampleMap : Map Int Int
exampleMap = fromList [Entry 3 300, Entry 1 100, Entry 4 400, Entry 2 200]

exampleLookupHit : Maybe Int
exampleLookupHit = lookup 3 exampleMap

exampleLookupMiss : Maybe Int
exampleLookupMiss = lookup 9 exampleMap

exampleMember : Bool
exampleMember = member 2 exampleMap

exampleDelete : Map Int Int
exampleDelete = delete 3 exampleMap
