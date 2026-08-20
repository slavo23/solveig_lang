-- | Takes at most n elements from the beginning of a list.
take : forall a. Int -> [a] -> [a]
take count xs =
    if count <= 0 then []
    else
        case xs of
            [] => []
            | x :: rest => x :: take (count - 1) rest

-- | Drops at most n elements from the beginning of a list.
drop : forall a. Int -> [a] -> [a]
drop count xs =
    if count <= 0 then xs
    else
        case xs of
            [] => []
            | _ :: rest => drop (count - 1) rest

-- | Takes elements while the predicate is true.
takeWhile : forall a. (a -> Bool) -> [a] -> [a]
takeWhile predicate [] = []
takeWhile predicate (x :: xs) =
    if predicate x then x :: takeWhile predicate xs else []

-- | Drops elements while the predicate is true.
dropWhile : forall a. (a -> Bool) -> [a] -> [a]
dropWhile predicate [] = []
dropWhile predicate (x :: xs) =
    if predicate x then dropWhile predicate xs else x :: xs

-- | Combines corresponding elements until either input is exhausted.
zipWith : forall a b c. (a -> b -> c) -> [a] -> [b] -> [c]
zipWith combine (x :: xs) (y :: ys) = combine x y :: zipWith combine xs ys
zipWith combine _ _ = []

-- | Pairs corresponding elements until either input is exhausted.
zip : forall a b. [a] -> [b] -> [Pair a b]
zip = zipWith MkPair

-- | Reverses the order of the elements in a list.
reverse : forall a. [a] -> [a]
reverse xs = reverseInto xs []

-- | Reverses a list into an accumulator.
reverseInto : forall a. [a] -> [a] -> [a]
reverseInto [] acc = acc
reverseInto (y :: ys) acc = reverseInto ys (y :: acc)

-- | Builds an ascending half-open integer range.
range : Int -> Int -> [Int]
range start end =
    if start >= end then []
    else rangeDown (end - 1) start []

-- | Builds an ascending inclusive integer range.
rangeInclusive : Int -> Int -> [Int]
rangeInclusive start end =
    if start > end then []
    else rangeDown end start []

-- | Returns the number of elements in a finite list.
length : forall a. [a] -> Int
length values = lengthInto values 0

lengthInto : forall a. [a] -> Int -> Int
lengthInto [] count = count
lengthInto (_ :: rest) count = lengthInto rest (count + 1)

-- | Reports whether a list contains no elements.
isEmpty : forall a. [a] -> Bool
isEmpty [] = True
isEmpty _ = False

-- | Builds an ascending range by counting down into an accumulator.
rangeDown : Int -> Int -> [Int] -> [Int]
rangeDown current start acc =
    if current < start then acc
    else if current == start then current :: acc
    else rangeDown (current - 1) start (current :: acc)
