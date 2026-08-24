export (Queue, empty, singleton, length, isEmpty, enqueue, peek, dequeue, fromList, toList)

import std.list(reverse)

-- | An immutable FIFO queue with amortized constant-time ends.
data Queue a = MkQueue Int [a] [a]

-- | Returns an empty queue.
empty : forall a. Queue a
empty = MkQueue 0 [] []

-- | Returns a one-element queue.
singleton : forall a. a -> Queue a
singleton value = MkQueue 1 [value] []

-- | Returns the number of queued elements in constant time.
length : forall a. Queue a -> Int
length (MkQueue count _ _) = count

-- | Reports whether the queue is empty.
isEmpty : forall a. Queue a -> Bool
isEmpty (MkQueue 0 _ _) = True
isEmpty _ = False

-- Okasaki, Purely Functional Data Structures, section 5.2: reversing the rear
-- list only when the front empties gives amortized constant-time FIFO ends.
normalize : forall a. Queue a -> Queue a
normalize (MkQueue count [] rear) = MkQueue count (reverse rear) []
normalize queue = queue

-- | Adds an element to the back in amortized constant time.
enqueue : forall a. Queue a -> a -> Queue a
enqueue (MkQueue count front rear) value =
    normalize (MkQueue (count + 1) front (value :: rear))

-- | Returns the front element without removing it.
peek : forall a. Queue a -> Maybe a
peek queue =
    case normalize queue of
        MkQueue _ [] _ => Nothing
        | MkQueue _ (value :: _) _ => Just value

-- | Removes and returns the front element when one exists.
dequeue : forall a. Queue a -> Maybe (a, (Queue a))
dequeue queue =
    case normalize queue of
        MkQueue _ [] _ => Nothing
        | MkQueue count (value :: front) rear =>
            Just (value, (normalize (MkQueue (count - 1) front rear)))

-- | Builds a queue in linear time.
fromList : forall a. [a] -> Queue a
fromList values = foldl enqueue empty values

-- | Returns elements from front to back in linear time.
toList : forall a. Queue a -> [a]
toList (MkQueue _ front rear) = front ++ reverse rear

instance Functor Queue where
    fmap transform queue = fromList (map transform (toList queue))

instance Eq a => Eq (Queue a) where
    (==) left right = toList left == toList right
