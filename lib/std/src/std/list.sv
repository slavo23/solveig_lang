export (
    take,
    drop,
    takeWhile,
    dropWhile,
    zipWith,
    zip,
    reverse,
    range,
    rangeInclusive,
    length,
    isEmpty
)

-- | Takes at most n elements from the beginning of a finite list.
-- |
-- | Runs in linear time in the returned prefix and does not consume the call
-- | stack. A non-positive count returns the empty list.
take : forall a. Int -> [a] -> [a]
take count values = reverse (takeInto count values [])

-- | Drops at most n elements from the beginning of a list.
drop : forall a. Int -> [a] -> [a]
drop count xs =
    if count <= 0
        then xs
        else case xs of
            [] => []
            | _ :: rest => drop (count - 1) rest

-- | Takes elements while the predicate is true, testing from left to right.
-- |
-- | Runs in linear time in the inspected prefix and does not consume the call
-- | stack.
takeWhile : forall a. (a -> Bool) -> [a] -> [a]
takeWhile predicate values = reverse (takeWhileInto predicate values [])

-- | Drops elements while the predicate is true.
dropWhile : forall a. (a -> Bool) -> [a] -> [a]
dropWhile predicate [] = []
dropWhile predicate (x :: xs) = if predicate x then dropWhile predicate xs else x :: xs

-- | Combines corresponding elements from left to right until an input ends.
-- |
-- | Runs in linear time and does not consume the call stack.
zipWith : forall a b c. (a -> b -> c) -> [a] -> [b] -> [c]
zipWith combine left right = reverse (zipWithInto combine left right [])

-- | Pairs corresponding elements until either input is exhausted.
zip : forall a b. [a] -> [b] -> [(a, b)]
zip = zipWith (,)

-- | Reverses the order of the elements in a list.
reverse : forall a. [a] -> [a]
reverse xs = reverseInto xs []

-- | Reverses a list into an accumulator.
reverseInto : forall a. [a] -> [a] -> [a]
reverseInto [] acc = acc
reverseInto (y :: ys) acc = reverseInto ys (y :: acc)

-- | Builds an ascending half-open integer range.
range : Int -> Int -> [Int]
range start end = if start >= end then [] else rangeDown (end - 1) start []

-- | Builds an ascending inclusive integer range.
rangeInclusive : Int -> Int -> [Int]
rangeInclusive start end = if start > end then [] else rangeDown end start []

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

takeInto : forall a. Int -> [a] -> [a] -> [a]
takeInto count values reversed =
    if count <= 0
        then reversed
        else
            case values of
                [] => reversed
                | value :: rest => takeInto (count - 1) rest (value :: reversed)

takeWhileInto : forall a. (a -> Bool) -> [a] -> [a] -> [a]
takeWhileInto predicate [] reversed = reversed
takeWhileInto predicate (value :: rest) reversed =
    if predicate value then takeWhileInto predicate rest (value :: reversed) else reversed

zipWithInto : forall a b c. (a -> b -> c) -> [a] -> [b] -> [c] -> [c]
zipWithInto combine (left :: leftRest) (right :: rightRest) reversed =
    zipWithInto combine leftRest rightRest (combine left right :: reversed)
zipWithInto combine _ _ reversed = reversed

-- | Builds an ascending range by counting down into an accumulator.
rangeDown : Int -> Int -> [Int] -> [Int]
rangeDown current start acc =
    if current < start
        then acc
        else if current == start
                then current :: acc
                else rangeDown (current - 1) start (current :: acc)
