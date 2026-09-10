export (
    Stream,
    StreamStep(..),
    StreamError(..),
    next,
    empty,
    singleton,
    unfold,
    map,
    mapError,
    filter,
    take,
    chunksOf,
    fold,
    forEach,
    toListLimited,
    fromList,
    fromText,
    fromBytes
)

import prelude.builtins
import prelude.prim(stringCharAtByte)
import std.bytes(bytesIndex, bytesLength)
import std.list(reverse)
import std.text(byteLength, charToCodePoint)

-- | One result from pulling a strict stream.
-- |
-- | StreamDone ends the stream. StreamYield returns exactly one value and the
-- | stream that owns the next pull. A pull never runs before next requests it.
data StreamStep scope e a = StreamDone | StreamYield a (Stream scope e a)

-- | A strict pull-based sequence whose resource lifetime is identified by scope.
-- |
-- | The representation is opaque. Resource producers introduce scope through
-- | a rank-2 callback, preventing the Stream value from escaping its owner.
-- | Resource-backed streams are single-consumer values: always continue with
-- | the remaining Stream returned by StreamYield, and never pull one value
-- | repeatedly or concurrently.
data Stream scope e a = Stream (IO e (StreamStep scope e a))

-- | A validation failure from a bounded Stream combinator.
data StreamError = InvalidChunkSize Int | InvalidStreamLimit Int | StreamLimitExceeded Int

instance Eq StreamError where
    (==) (InvalidChunkSize left) (InvalidChunkSize right) = left == right
    (==) (InvalidStreamLimit left) (InvalidStreamLimit right) = left == right
    (==) (StreamLimitExceeded left) (StreamLimitExceeded right) = left == right
    (==) _ _ = False

instance Show StreamError where
    show (InvalidChunkSize size) =
        "invalid stream chunk size " <> show size <> "; expected a positive Int"
    show (InvalidStreamLimit limit) =
        "invalid stream collection limit " <> show limit <> "; expected a non-negative Int"
    show (StreamLimitExceeded limit) = "stream contains more than " <> show limit <> " elements"

instance Functor (Stream scope e) where
    fmap = map

-- | Requests at most one value from a stream.
-- |
-- | The returned IO action performs the upstream effects for this pull only.
-- | Continue by pulling the remaining Stream from StreamYield. Pulling the
-- | same resource-backed Stream twice repeats or races its underlying action.
next : forall scope e a. Stream scope e a -> IO e (StreamStep scope e a)
next (Stream action) = action

-- | Returns a neutral-scope stream that is complete and performs no effects.
empty : forall e a. Stream () e a
empty = emptyScoped

emptyScoped : forall scope e a. Stream scope e a
emptyScoped = Stream (pureIO StreamDone)

-- | Returns a neutral-scope stream that yields one value and then completes.
singleton : forall e a. a -> Stream () e a
singleton value = Stream (pureIO (StreamYield value emptyScoped))

-- | Builds a demand-driven stream from an effectful state transition.
-- |
-- | Nothing completes the stream. Just (value, state) emits one value and
-- | retains the new state for the next pull. No transition runs ahead of next.
-- | The source owns no external lifetime and therefore uses the neutral scope ().
unfold : forall e state a. (state -> IO e (Maybe (a, state))) -> state -> Stream () e a
unfold = unfoldScoped

-- Package-internal constructor used by rank-2 resource producers. Keeping the
-- scope universally quantified preserves the public escape-safety boundary.
unfoldScoped :
    forall scope e state a. (state -> IO e (Maybe (a, state))) -> state -> Stream scope e a
unfoldScoped step state = Stream (fmap (unfoldScopedStep step) (step state))

unfoldScopedStep :
    forall scope e state a. (state -> IO e (Maybe (a, state))) ->
    Maybe (a, state) -> StreamStep scope e a
unfoldScopedStep step Nothing = StreamDone
unfoldScopedStep step (Just (value, state)) = StreamYield value (unfoldScoped step state)

-- | Transforms each value in order without prefetching or extra buffering.
map : forall scope e a b. (a -> b) -> Stream scope e a -> Stream scope e b
map transform stream = Stream (fmap (mapStep transform) (next stream))

mapStep : forall scope e a b. (a -> b) -> StreamStep scope e a -> StreamStep scope e b
mapStep transform StreamDone = StreamDone
mapStep transform (StreamYield value rest) = StreamYield (transform value) (map transform rest)

-- | Transforms typed pull failures without changing values or pull order.
mapError : forall scope e f a. (e -> f) -> Stream scope e a -> Stream scope f a
mapError transform stream =
    Stream (fmap (mapErrorStep transform) (prelude.builtins.mapError transform (next stream)))

mapErrorStep : forall scope e f a. (e -> f) -> StreamStep scope e a -> StreamStep scope f a
mapErrorStep transform StreamDone = StreamDone
mapErrorStep transform (StreamYield value rest) = StreamYield value (mapError transform rest)

-- | Keeps values satisfying a pure predicate while preserving order.
-- |
-- | It buffers at most one accepted value. One next may pull repeatedly until
-- | a value is accepted or upstream completes; no pull occurs after that.
filter : forall scope e a. (a -> Bool) -> Stream scope e a -> Stream scope e a
filter predicate stream = Stream (filterNext predicate stream)

filterNext : forall scope e a. (a -> Bool) -> Stream scope e a -> IO e (StreamStep scope e a)
filterNext predicate stream = do
    step <- next stream
    case step of
        StreamDone => pureIO StreamDone
        | StreamYield value rest =>
            if predicate value
                then pureIO (StreamYield value (filter predicate rest))
                else filterNext predicate rest

-- | Yields at most count values and performs no extra upstream pull.
-- |
-- | A non-positive count is an empty stream. For a scoped resource, consuming
-- | the result and returning from the owner callback releases upstream early.
take : forall scope e a. Int -> Stream scope e a -> Stream scope e a
take count stream = if count <= 0 then emptyScoped else Stream (takeNext count stream)

takeNext : forall scope e a. Int -> Stream scope e a -> IO e (StreamStep scope e a)
takeNext count stream = do
    step <- next stream
    case step of
        StreamDone => pureIO StreamDone
        | StreamYield value rest =>
            let remaining =
                count - 1
            in pureIO
                (StreamYield value (if remaining == 0 then emptyScoped else take remaining rest))

-- | Groups consecutive values into non-empty lists of at most size elements.
-- |
-- | A positive size yields a stream whose order is unchanged and whose next
-- | buffers at most size values. Invalid sizes return InvalidChunkSize without
-- | pulling upstream.
chunksOf : forall scope e a. Int -> Stream scope e a -> Either StreamError (Stream scope e [a])
chunksOf size stream =
    if size <= 0 then Left (InvalidChunkSize size) else Right (chunksOfValid size stream)

chunksOfValid : forall scope e a. Int -> Stream scope e a -> Stream scope e [a]
chunksOfValid size stream = Stream (pullChunk size stream 0 [])

pullChunk : forall scope e a. Int -> Stream scope e a -> Int -> [a] -> IO e (StreamStep scope e [a])
pullChunk size stream count reversed = do
    step <- next stream
    case step of
        StreamDone =>
            if count == 0
                then pureIO StreamDone
                else pureIO (StreamYield (reverse reversed) emptyScoped)
        | StreamYield value rest =>
            let nextCount =
                count + 1
            in let nextReversed =
                value :: reversed
            in if nextCount == size
                then pureIO (StreamYield (reverse nextReversed) (chunksOfValid size rest))
                else pullChunk size rest nextCount nextReversed

-- | Strictly folds values from left to right using constant collection space.
-- |
-- | The fold is stack-safe through IO's managed continuation loop and stops
-- | immediately on a typed failure, defect, or interruption.
fold : forall scope e a state. (state -> a -> state) -> state -> Stream scope e a -> IO e state
fold combine state stream = do
    step <- next stream
    case step of
        StreamDone => pureIO state
        | StreamYield value rest => fold combine (combine state value) rest

-- | Runs one effect for each value sequentially and in stream order.
-- |
-- | It buffers at most one value and stops immediately on failure, defect, or
-- | interruption.
forEach : forall scope e a. (a -> IO e ()) -> Stream scope e a -> IO e ()
forEach action stream = do
    step <- next stream
    case step of
        StreamDone => pureIO ()
        | StreamYield value rest => do
            action value
            forEach action rest

-- | Collects at most limit values into a finite List.
-- |
-- | A negative limit returns InvalidStreamLimit without pulling. The operation
-- | retains at most limit values and pulls one additional value only to prove
-- | StreamLimitExceeded; order is preserved and upstream is not drained.
toListLimited : forall scope e a. Int -> Stream scope e a -> IO e (Either StreamError [a])
toListLimited limit stream =
    if limit < 0
        then pureIO (Left (InvalidStreamLimit limit))
        else collectLimited limit limit stream []

collectLimited :
    forall scope e a. Int -> Int -> Stream scope e a -> [a] -> IO e (Either StreamError [a])
collectLimited limit remaining stream reversed = do
    step <- next stream
    case step of
        StreamDone => pureIO (Right (reverse reversed))
        | StreamYield value rest =>
            if remaining == 0
                then pureIO (Left (StreamLimitExceeded limit))
                else collectLimited limit (remaining - 1) rest (value :: reversed)

-- | Streams an existing finite List without copying it or reading ahead.
-- | Finite adapters own no external lifetime and use the neutral scope ().
fromList : forall e a. [a] -> Stream () e a
fromList [] = emptyScoped
fromList (value :: rest) = Stream (pureIO (StreamYield value (fromList rest)))

-- | Streams Unicode scalar values directly from packed UTF-8 text.
-- |
-- | Each pull advances by the emitted scalar's encoded byte width and
-- | allocates no intermediate `[Char]`.
fromText : forall e. String -> Stream () e Char
fromText text = unfold (textStep text) (0, byteLength text)

textStep : forall e. String -> (Int, Int) -> IO e (Maybe (Char, (Int, Int)))
textStep text (position, total) =
    if position >= total
        then pureIO Nothing
        else
            case stringCharAtByte text position of
                Nothing => error "std.stream.fromText lost a UTF-8 boundary"
                | Just character =>
                    pureIO (Just (character, (position + charByteLength character, total)))

charByteLength : Char -> Int
charByteLength character =
    let point =
        charToCodePoint character
    in if point <= 127 then 1 else if point <= 2047 then 2 else if point <= 65535 then 3 else 4

-- | Streams integer byte values from an immutable Bytes value in constant time per pull.
fromBytes : forall e. Bytes -> Stream () e Int
fromBytes bytes = unfold (bytesStep bytes) (0, bytesLength bytes)

bytesStep : forall e. Bytes -> (Int, Int) -> IO e (Maybe (Int, (Int, Int)))
bytesStep bytes (position, total) =
    if position >= total
        then pureIO Nothing
        else
            case bytesIndex bytes position of
                Nothing => error "std.stream.fromBytes lost an in-range byte index"
                | Just byte => pureIO (Just (byte, (position + 1, total)))
