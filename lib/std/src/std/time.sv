export (sleep, getMonotonicTime, getSystemTime, measure)

import prelude.prim

-- | Suspends the current thread for at least the requested milliseconds.
sleep : Int -> Task ()
sleep milliseconds = prelude.prim.sleep milliseconds

-- | Returns nanoseconds from an arbitrary runtime-local monotonic origin.
getMonotonicTime : Task Int
getMonotonicTime = prelude.prim.getMonotonicTime

-- | Returns signed nanoseconds since the Unix epoch.
getSystemTime : Task Int
getSystemTime = prelude.prim.getSystemTime

-- | Runs an IO operation and returns its monotonic elapsed nanoseconds with its
-- | result.
measure : forall a. Task a -> Task (Int, a)
measure operation = do
    started <- getMonotonicTime
    value <- operation
    finished <- getMonotonicTime
    prelude.prim.pureIO (finished - started, value)
