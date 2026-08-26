import std.hashmap
import std.hashset
import std.list(length, rangeInclusive, reverse)
import std.map

data CollisionKey = CollisionKey Int

data HashOnly = HashOnly Int

instance Eq CollisionKey where
    (==) (CollisionKey left) (CollisionKey right) = left == right

instance PartialOrd CollisionKey where
    partialCompare (CollisionKey left) (CollisionKey right) =
        partialCompare left right

instance Ord CollisionKey where
    compare (CollisionKey left) (CollisionKey right) = compare left right

instance Show CollisionKey where
    show (CollisionKey value) = "Key " <> show value

-- Every value deliberately occupies one collision bucket.
instance Hashable CollisionKey where
    hashWithSalt salt _ = salt

instance Eq HashOnly where
    (==) (HashOnly left) (HashOnly right) = left == right

instance Hashable HashOnly where
    hashWithSalt salt (HashOnly value) = hashWithSalt salt value

collisionEntries : Int -> [(CollisionKey, Int)]
collisionEntries count =
    map (fun value => (CollisionKey value, value * 10)) (rangeInclusive 1 count)

collisionKeys : Int -> [CollisionKey]
collisionKeys count = map CollisionKey (rangeInclusive 1 count)

buildLarge : Int -> std.hashmap.HashMap Int Int -> std.hashmap.HashMap Int Int
buildLarge 0 dictionary = dictionary
buildLarge count dictionary =
    buildLarge
        (count - 1)
        (std.hashmap.insert count (count * 2) dictionary)

applyModel :
    Int ->
    std.hashmap.HashMap Int Int ->
    std.map.Map Int Int ->
    (std.hashmap.HashMap Int Int, std.map.Map Int Int)
applyModel 0 hashed ordered = (hashed, ordered)
applyModel count hashed ordered =
    let key = mod (count * 37) 257 in
    if mod count 5 == 0 then
        applyModel
            (count - 1)
            (std.hashmap.delete key hashed)
            (std.map.delete key ordered)
    else
        applyModel
            (count - 1)
            (std.hashmap.insert key count hashed)
            (std.map.insert key count ordered)

modelAgreement : Bool
modelAgreement =
    case applyModel 2000 std.hashmap.empty std.map.empty of
        (hashed, ordered) =>
            std.hashmap.toAscList hashed == std.map.toAscList ordered

minimumInt : Int
minimumInt = (0 - 9223372036854775807) - 1

positive : Int -> Maybe Int
positive value = if value > 0 then Just value else Nothing

main : Task ()
main = do
    let entries = collisionEntries 256
    let collisions = std.hashmap.fromList entries
    let collisionsWithoutMiddle = std.hashmap.delete (CollisionKey 128) collisions
    printLine (show (std.hashmap.size collisions))
    printLine (show (std.hashmap.lookup (CollisionKey 256) collisions))
    printLine (show (std.hashmap.lookup (CollisionKey 128) collisionsWithoutMiddle))
    printLine (show (std.hashmap.lookup (CollisionKey 128) collisions))
    printLine (show (length (std.hashmap.toList collisions)))
    printLine (show (std.hashmap.toAscList collisions == entries))
    let replaced = std.hashmap.insert (CollisionKey 128) 9999 collisions
    printLine (show (std.hashmap.size replaced == std.hashmap.size collisions))
    printLine (show (std.hashmap.lookup (CollisionKey 128) replaced))
    printLine (show (std.hashmap.lookup (CollisionKey 128) collisions))

    let forward = std.hashmap.fromList entries
    let backward = std.hashmap.fromList (reverse entries)
    printLine (show (forward == backward))
    printLine (show (hash forward == hash backward))
    printLine (show (show forward == show backward))
    let hashOnlyForward = std.hashmap.fromList [(HashOnly 1, 10), (HashOnly 2, 20)]
    let hashOnlyBackward = std.hashmap.fromList [(HashOnly 2, 20), (HashOnly 1, 10)]
    printLine (show (hash hashOnlyForward == hash hashOnlyBackward))
    printLine (show (std.hashmap.toAscList (fmap ((+) 1) forward) ==
        map (fun (key, value) => (key, value + 1)) (std.hashmap.toAscList forward)))
    printLine (show (foldl (+) 0 forward))
    printLine (show (traverse positive (std.hashmap.fromList [(1, 10), (2, 20)])))
    printLine (show (traverse positive (std.hashmap.fromList [(1, 10), (2, 0)])))
    printLine (show modelAgreement)

    let signed = std.hashmap.fromList [(minimumInt, 1), (0 - 1, 2), (31, 3)]
    printLine (show (std.hashmap.toAscList signed))
    printLine (show (std.hashmap.lookup (0 - 1) signed))

    let large = buildLarge 10000 std.hashmap.empty
    let smaller = std.hashmap.delete 5000 large
    printLine (show (std.hashmap.size large))
    printLine (show (std.hashmap.lookup 9999 large))
    printLine (show (std.hashmap.lookup 5000 smaller))
    printLine (show (std.hashmap.lookup 5000 large))
    printLine (show (length (std.hashmap.toList large)))

    let first = std.hashset.fromList (collisionKeys 200)
    let second = std.hashset.fromList (map CollisionKey (rangeInclusive 101 300))
    printLine (show (std.hashset.size (std.hashset.union first second)))
    printLine (show (std.hashset.size (std.hashset.intersection first second)))
    printLine (show (std.hashset.size (std.hashset.difference first second)))
    printLine (show (foldl (+) 0 (std.hashset.fromList [1, 2, 3, 3])))
    printLine (show (first == std.hashset.fromList (reverse (collisionKeys 200))))
    printLine (show (hash first == hash (std.hashset.fromList (reverse (collisionKeys 200)))))
    printLine (show (show first == show (std.hashset.fromList (reverse (collisionKeys 200)))))
