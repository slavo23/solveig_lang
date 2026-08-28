export (
    Duration, Instant, SystemTime, nanoseconds, microseconds, milliseconds, seconds,
    durationToNanoseconds, durationToMicroseconds, durationToMilliseconds, durationToSeconds,
    instantDifference, systemTimeDifference, systemTimeNanoseconds, sleep,
    getMonotonicTime, getSystemTime, measure
)

private (maximumInt, minimumInt, saturatingScale, saturatingDifference, sleepMilliseconds)

import prelude.prim(*)

-- | A signed span of time stored as nanoseconds.
-- |
-- | Signed durations are useful for clock differences. Sleeping for a
-- | non-positive duration completes immediately, matching the host primitive.
data Duration = Duration Int

-- | A reading from the runtime-local monotonic clock.
data Instant = Instant Int

-- | Signed nanoseconds since the Unix epoch from the wall clock.
data SystemTime = SystemTime Int

instance Eq Duration where
    (==) (Duration left) (Duration right) = left == right

instance PartialOrd Duration where
    partialCompare (Duration left) (Duration right) = partialCompare left right

instance Ord Duration where
    compare (Duration left) (Duration right) = compare left right

instance Show Duration where
    show (Duration value) = show value <> "ns"

instance Hashable Duration where
    hashWithSalt salt (Duration value) = hashWithSalt salt value

instance Eq Instant where
    (==) (Instant left) (Instant right) = left == right

instance PartialOrd Instant where
    partialCompare (Instant left) (Instant right) = partialCompare left right

instance Ord Instant where
    compare (Instant left) (Instant right) = compare left right

instance Show Instant where
    show (Instant value) = "Instant " <> show value <> "ns"

instance Hashable Instant where
    hashWithSalt salt (Instant value) = hashWithSalt salt value

instance Eq SystemTime where
    (==) (SystemTime left) (SystemTime right) = left == right

instance PartialOrd SystemTime where
    partialCompare (SystemTime left) (SystemTime right) = partialCompare left right

instance Ord SystemTime where
    compare (SystemTime left) (SystemTime right) = compare left right

instance Show SystemTime where
    show (SystemTime value) = "SystemTime " <> show value <> "ns"

instance Hashable SystemTime where
    hashWithSalt salt (SystemTime value) = hashWithSalt salt value

-- | Constructs a duration from nanoseconds.
nanoseconds : Int -> Duration
nanoseconds = Duration

-- | Constructs a duration from microseconds, saturating at the nanosecond bounds.
microseconds : Int -> Duration
microseconds value = Duration (saturatingScale 1000 value)

-- | Constructs a duration from milliseconds, saturating at the nanosecond bounds.
milliseconds : Int -> Duration
milliseconds value = Duration (saturatingScale 1000000 value)

-- | Constructs a duration from seconds, saturating at the nanosecond bounds.
seconds : Int -> Duration
seconds value = Duration (saturatingScale 1000000000 value)

-- | Returns a duration as nanoseconds.
durationToNanoseconds : Duration -> Int
durationToNanoseconds (Duration value) = value

-- | Returns whole microseconds, truncating toward zero.
durationToMicroseconds : Duration -> Int
durationToMicroseconds (Duration value) = div value 1000

-- | Returns whole milliseconds, truncating toward zero.
durationToMilliseconds : Duration -> Int
durationToMilliseconds (Duration value) = div value 1000000

-- | Returns whole seconds, truncating toward zero.
durationToSeconds : Duration -> Int
durationToSeconds (Duration value) = div value 1000000000

-- | Subtracts the first monotonic reading from the second, saturating at the bounds.
instantDifference : Instant -> Instant -> Duration
instantDifference (Instant started) (Instant finished) =
    Duration (saturatingDifference started finished)

-- | Subtracts the first wall-clock reading from the second, saturating at the bounds.
systemTimeDifference : SystemTime -> SystemTime -> Duration
systemTimeDifference (SystemTime started) (SystemTime finished) =
    Duration (saturatingDifference started finished)

-- | Returns signed nanoseconds since the Unix epoch.
systemTimeNanoseconds : SystemTime -> Int
systemTimeNanoseconds (SystemTime value) = value

-- Package-internal conversion for nanoseconds returned by host primitives.
systemTimeFromNanoseconds : Int -> SystemTime
systemTimeFromNanoseconds = SystemTime

-- | Suspends the current thread for at least the requested duration.
sleep : Duration -> Task ()
sleep duration = prelude.prim.sleep (sleepMilliseconds duration)

sleepMilliseconds : Duration -> Int
sleepMilliseconds (Duration value) =
    if value <= 0 then 0
    else
        let whole = div value 1000000 in
        if mod value 1000000 == 0 then whole else whole + 1

maximumInt : Int
maximumInt = 9223372036854775807

minimumInt : Int
minimumInt = (-9223372036854775807) - 1

saturatingScale : Int -> Int -> Int
saturatingScale factor value =
    if value > div maximumInt factor then maximumInt
    else if value < div minimumInt factor then minimumInt
    else value * factor

saturatingDifference : Int -> Int -> Int
saturatingDifference started finished =
    if started < 0 then
        if finished > maximumInt + started then maximumInt
        else finished - started
    else if started > 0 then
        if finished < minimumInt + started then minimumInt
        else finished - started
    else finished

-- | Returns a reading from an arbitrary runtime-local monotonic origin.
getMonotonicTime : forall e. IO e Instant
getMonotonicTime = fmap Instant prelude.prim.getMonotonicTime

-- | Returns the current wall-clock time relative to the Unix epoch.
getSystemTime : forall e. IO e SystemTime
getSystemTime = fmap SystemTime prelude.prim.getSystemTime

-- | Runs an operation and returns its monotonic elapsed duration with its result.
measure : forall e a. IO e a -> IO e (Duration, a)
measure operation = do
    started <- getMonotonicTime
    value <- operation
    finished <- getMonotonicTime
    prelude.prim.pureIO (instantDifference started finished, value)
