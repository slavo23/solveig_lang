-- | Takes at most n elements from the beginning of a list.
take : forall a. Int -> [a] -> [a]
take 0 _ = []
take _ [] = []
take count (x :: xs) = x :: take (count - 1) xs

-- | Drops at most n elements from the beginning of a list.
drop : forall a. Int -> [a] -> [a]
drop 0 xs = xs
drop _ [] = []
drop count (x :: xs) = drop (count - 1) xs

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

-- | Builds an ascending integer range.
range : Int -> Int -> Bool -> [Int]
range start end inclusive =
    if inclusive then
        rangeDown end start []
    else
        rangeDown (end - 1) start []

-- | Builds an ascending range by counting down into an accumulator.
rangeDown : Int -> Int -> [Int] -> [Int]
rangeDown current start acc =
    if current < start then acc
    else rangeDown (current - 1) start (current :: acc)
