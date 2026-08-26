export (Vector, empty, singleton, length, isEmpty, snoc, index, update, fromList, toList)

-- | An immutable sequence with logarithmic indexing, updates, and right append.
-- |
-- | The balanced tree is persistent: updates share every untouched branch.
data Vector a =
    VectorEmpty
  | VectorLeaf a
  | VectorBranch Int Int (Vector a) (Vector a)

-- | Returns an empty vector.
empty : forall a. Vector a
empty = VectorEmpty

-- | Returns a one-element vector.
singleton : forall a. a -> Vector a
singleton value = VectorLeaf value

-- | Returns the number of elements in constant time.
length : forall a. Vector a -> Int
length VectorEmpty = 0
length (VectorLeaf _) = 1
length (VectorBranch _ count _ _) = count

-- | Reports whether a vector has no elements.
isEmpty : forall a. Vector a -> Bool
isEmpty VectorEmpty = True
isEmpty _ = False

vectorHeight : forall a. Vector a -> Int
vectorHeight VectorEmpty = 0
vectorHeight (VectorLeaf _) = 1
vectorHeight (VectorBranch height _ _ _) = height

makeBranch : forall a. Vector a -> Vector a -> Vector a
makeBranch VectorEmpty right = right
makeBranch left VectorEmpty = left
makeBranch left right =
    VectorBranch
        (1 + maxOrd (vectorHeight left) (vectorHeight right))
        (length left + length right)
        left
        right

-- Adelson-Velsky and Landis (1962): restore the height invariant with a
-- single or double rotation after changing one spine.
balance : forall a. Vector a -> Vector a -> Vector a
balance left right =
    let leftHeight = vectorHeight left in
    let rightHeight = vectorHeight right in
    if leftHeight > rightHeight + 1 then
        case left of
            VectorBranch _ _ leftLeft leftRight =>
                if vectorHeight leftLeft >= vectorHeight leftRight then
                    makeBranch leftLeft (makeBranch leftRight right)
                else
                    case leftRight of
                        VectorBranch _ _ middleLeft middleRight =>
                            makeBranch
                                (makeBranch leftLeft middleLeft)
                                (makeBranch middleRight right)
                        | _ => makeBranch left right
            | _ => makeBranch left right
    else if rightHeight > leftHeight + 1 then
        case right of
            VectorBranch _ _ rightLeft rightRight =>
                if vectorHeight rightRight >= vectorHeight rightLeft then
                    makeBranch (makeBranch left rightLeft) rightRight
                else
                    case rightLeft of
                        VectorBranch _ _ middleLeft middleRight =>
                            makeBranch
                                (makeBranch left middleLeft)
                                (makeBranch middleRight rightRight)
                        | _ => makeBranch left right
            | _ => makeBranch left right
    else
        makeBranch left right

-- | Adds an element to the end in logarithmic time.
snoc : forall a. Vector a -> a -> Vector a
snoc VectorEmpty value = VectorLeaf value
snoc (VectorLeaf oldValue) value = makeBranch (VectorLeaf oldValue) (VectorLeaf value)
snoc (VectorBranch _ _ left right) value = balance left (snoc right value)

-- | Returns the element at a zero-based index in logarithmic time.
index : forall a. Vector a -> Int -> Maybe a
index vector offset =
    if offset < 0 then Nothing
    else if offset >= length vector then Nothing
    else indexTree vector offset

indexTree : forall a. Vector a -> Int -> Maybe a
indexTree VectorEmpty _ = Nothing
indexTree (VectorLeaf value) 0 = Just value
indexTree (VectorLeaf _) _ = Nothing
indexTree (VectorBranch _ _ left right) offset =
    let leftLength = length left in
    if offset < leftLength then indexTree left offset
    else indexTree right (offset - leftLength)

-- | Replaces an element at a zero-based index, sharing untouched branches.
update : forall a. Vector a -> Int -> a -> Maybe (Vector a)
update vector offset value =
    if offset < 0 then Nothing
    else if offset >= length vector then Nothing
    else updateTree vector offset value

updateTree : forall a. Vector a -> Int -> a -> Maybe (Vector a)
updateTree VectorEmpty _ _ = Nothing
updateTree (VectorLeaf _) 0 value = Just (VectorLeaf value)
updateTree (VectorLeaf _) _ _ = Nothing
updateTree (VectorBranch _ _ left right) offset value =
    let leftLength = length left in
    if offset < leftLength then
        case updateTree left offset value of
            Nothing => Nothing
            | Just updatedLeft => Just (makeBranch updatedLeft right)
    else
        case updateTree right (offset - leftLength) value of
            Nothing => Nothing
            | Just updatedRight => Just (makeBranch left updatedRight)

-- | Builds a balanced vector in linear time.
fromList : forall a. [a] -> Vector a
fromList values = buildVector (map VectorLeaf values)

-- Pairing adjacent trees builds each level once, so the total construction
-- work is linear while preserving source order.
buildVector : forall a. [Vector a] -> Vector a
buildVector [] = VectorEmpty
buildVector (vector :: []) = vector
buildVector vectors = buildVector (pairLevel vectors)

pairLevel : forall a. [Vector a] -> [Vector a]
pairLevel [] = []
pairLevel (vector :: []) = vector :: []
pairLevel (left :: right :: rest) = (makeBranch left right) :: pairLevel rest

-- | Converts a vector to a list in linear time.
toList : forall a. Vector a -> [a]
toList vector = toListInto vector []

toListInto : forall a. Vector a -> [a] -> [a]
toListInto VectorEmpty acc = acc
toListInto (VectorLeaf value) acc = value :: acc
toListInto (VectorBranch _ _ left right) acc =
    toListInto left (toListInto right acc)

instance Functor Vector where
    fmap transform VectorEmpty = VectorEmpty
    fmap transform (VectorLeaf value) = VectorLeaf (transform value)
    fmap transform (VectorBranch height count left right) =
        VectorBranch height count (fmap transform left) (fmap transform right)

instance Foldable Vector where
    foldr combine initial vector = foldr combine initial (toList vector)
    foldl combine initial vector = foldl combine initial (toList vector)

instance Traversable Vector where
    traverse _ VectorEmpty = pure VectorEmpty
    traverse transform (VectorLeaf value) = fmap VectorLeaf (transform value)
    traverse transform (VectorBranch height count left right) =
        liftA2
            (VectorBranch height count)
            (traverse transform left)
            (traverse transform right)

instance Eq a => Eq (Vector a) where
    (==) left right = toList left == toList right

instance Show a => Show (Vector a) where
    show vector = "fromList " <> show (toList vector)

instance Hashable a => Hashable (Vector a) where
    hashWithSalt salt vector = hashWithSalt salt (toList vector)
