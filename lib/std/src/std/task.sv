export (
    catchDefect, mask, finally, bracket, withThread, race, concurrently, timeout,
    ConcurrentError(..), concurrentHostError, mapConcurrentlyBounded
)

import prelude.prim(catchDefectIO, maskIO, finallyIO, restoreMaskIO, raiseDefectIO)
import std.channel(*)
import std.list(*)
import std.map(*)
import std.time(sleep)
import std.thread(spawn, killThread)

-- | Distinguishes a pool/channel failure from a failure returned by one worker.
data ConcurrentError e =
    ConcurrentInfrastructure HostError
  | ConcurrentWorker e

instance Eq e => Eq (ConcurrentError e) where
    (==) (ConcurrentInfrastructure left) (ConcurrentInfrastructure right) = left == right
    (==) (ConcurrentWorker left) (ConcurrentWorker right) = left == right
    (==) _ _ = False

instance Show e => Show (ConcurrentError e) where
    show (ConcurrentInfrastructure hostError) =
        "concurrency infrastructure: " <> show hostError
    show (ConcurrentWorker workerError) =
        "concurrent worker: " <> show workerError

-- | Collapses a pool running Task workers back to Task at an explicit boundary.
concurrentHostError : ConcurrentError HostError -> HostError
concurrentHostError (ConcurrentInfrastructure hostError) = hostError
concurrentHostError (ConcurrentWorker hostError) = hostError

-- | Handles defects raised while executing an IO action's installed scope.
catchDefect : forall e a. IO e a -> (Defect -> IO e a) -> IO e a
catchDefect action handler = prelude.prim.catchDefectIO action handler

-- | Defers interruption while an action runs, except while it is blocked.
mask : forall e a. IO e a -> IO e a
mask action = prelude.prim.maskIO action

-- | Runs cleanup after an action succeeds or raises a typed error.
finally : forall e a b. IO e a -> IO e b -> IO e a
finally action cleanup =
    prelude.prim.maskIO
        (prelude.prim.finallyIO
            (prelude.prim.restoreMaskIO action)
            cleanup)

-- | Acquires a resource and releases it after its use action finishes.
bracket : forall e resource cleanup a. IO e resource -> (resource -> IO e cleanup) -> (resource -> IO e a) -> IO e a
bracket acquire release use =
    mask
        (bindIO acquire (fun resource =>
            prelude.prim.finallyIO
                (prelude.prim.restoreMaskIO (use resource))
                (release resource)))

-- | Runs a child action within a scope and waits for its cancellation on exit.
-- | Peyton Jones, Gordon, and Finne, "Concurrent Haskell" (POPL 1996), section
-- | 3.2, derives coordination abstractions from a private typed channel.
-- | Control.Concurrent.Async documents `withAsync` as bracketed spawn and cancel:
-- | https://hackage.haskell.org/package/async/docs/Control-Concurrent-Async.html
withThread : forall e a b. IO e a -> (ThreadId -> IO e b) -> IO e b
withThread action body =
    bindIO newChan (fun started =>
        bindIO newChan (fun finished =>
            bracket
                (spawn
                    (finally
                        (bindIO
                            (send started ())
                            (fun firstReady =>
                                bindIO
                                    (send started ())
                                    (fun secondReady =>
                                        catchError
                                            (bindIO
                                                (prelude.prim.restoreMaskIO action)
                                                (fun value => pureIO ()))
                                            (fun actionError => pureIO ()))))
                        (send finished ())))
                (fun thread =>
                    catchError
                        (bindIO
                            (recv started)
                            (fun ready =>
                                bindIO
                                    (killThread thread)
                                    (fun killed => recv finished)))
                        (fun channelError =>
                            error "structured concurrency internal channel closed"))
                (fun thread =>
                    bindIO
                        (catchError
                            (recv started)
                            (fun channelError =>
                                error "structured concurrency internal channel closed"))
                        (fun ready => body thread))))

-- | Runs two actions and returns the first result, cancelling the loser.
-- Control.Concurrent.Async specifies `race` as two scoped children followed by
-- a first-completion wait; the private channel is Solveig's wait primitive:
-- https://hackage.haskell.org/package/async/docs/Control-Concurrent-Async.html#v:race
race : forall e a b. IO e a -> IO e b -> IO e (Either a b)
race left right =
    bindIO newChan (fun outcomes =>
        withThread
            (catchError
                (catchDefect
                    (bindIO
                        (catchError
                            (bindIO left (fun value => pureIO (Right value)))
                            (fun typedError => pureIO (Left typedError)))
                        (fun outcome => send outcomes (Left (Right outcome))))
                    (fun defect => send outcomes (Left (Left defect))))
                (fun channelError =>
                    error "structured concurrency internal channel closed"))
            (fun leftThread =>
            withThread
                (catchError
                    (catchDefect
                        (bindIO
                            (catchError
                                (bindIO right (fun value => pureIO (Right value)))
                                (fun typedError => pureIO (Left typedError)))
                            (fun outcome => send outcomes (Right (Right outcome))))
                        (fun defect => send outcomes (Right (Left defect))))
                    (fun channelError =>
                        error "structured concurrency internal channel closed"))
                (fun rightThread =>
                    bindIO
                        (catchError
                            (recv outcomes)
                            (fun channelError =>
                                error "structured concurrency internal channel closed"))
                        (fun winner =>
                            case winner of
                                Left leftOutcome =>
                                    case leftOutcome of
                                        Left defect => prelude.prim.raiseDefectIO defect
                                        | Right typedOutcome =>
                                            case typedOutcome of
                                                Left typedError => failIO typedError
                                                | Right value => pureIO (Left value)
                                | Right rightOutcome =>
                                    case rightOutcome of
                                        Left defect => prelude.prim.raiseDefectIO defect
                                        | Right typedOutcome =>
                                            case typedOutcome of
                                                Left typedError => failIO typedError
                                                | Right value => pureIO (Right value)))))

-- | Runs two actions and returns both results, cancelling either after failure.
-- | Control.Concurrent.Async's `concurrently` waits for both scoped children but
-- | cancels the peer as soon as one child fails:
-- | https://hackage.haskell.org/package/async/docs/Control-Concurrent-Async.html#v:concurrently
concurrently : forall e a b. IO e a -> IO e b -> IO e (a, b)
concurrently left right =
    bindIO newChan (fun outcomes =>
        withThread
            (catchError
                (catchDefect
                    (bindIO
                        (catchError
                            (bindIO left (fun value => pureIO (Right value)))
                            (fun typedError => pureIO (Left typedError)))
                        (fun outcome => send outcomes (Left (Right outcome))))
                    (fun defect => send outcomes (Left (Left defect))))
                (fun channelError =>
                    error "structured concurrency internal channel closed"))
            (fun leftThread =>
            withThread
                (catchError
                    (catchDefect
                        (bindIO
                            (catchError
                                (bindIO right (fun value => pureIO (Right value)))
                                (fun typedError => pureIO (Left typedError)))
                            (fun outcome => send outcomes (Right (Right outcome))))
                        (fun defect => send outcomes (Right (Left defect))))
                    (fun channelError =>
                        error "structured concurrency internal channel closed"))
                (fun rightThread =>
                    bindIO
                        (catchError
                            (recv outcomes)
                            (fun channelError =>
                                error "structured concurrency internal channel closed"))
                        (fun first =>
                            let firstAction =
                                case first of
                                    Left (Left defect) => prelude.prim.raiseDefectIO defect
                                    | Left (Right (Left typedError)) => failIO typedError
                                    | Left (Right (Right leftValue)) => pureIO (Left leftValue)
                                    | Right (Left defect) => prelude.prim.raiseDefectIO defect
                                    | Right (Right (Left typedError)) => failIO typedError
                                    | Right (Right (Right rightValue)) => pureIO (Right rightValue)
                            in bindIO firstAction
                                (fun firstValue =>
                                    bindIO
                                        (catchError
                                            (recv outcomes)
                                            (fun channelError =>
                                                error "structured concurrency internal channel closed"))
                                        (fun second =>
                                            case (firstValue, second) of
                                                (Left leftValue, Right (Left defect)) =>
                                                    prelude.prim.raiseDefectIO defect
                                                | (Left leftValue, Right (Right (Left typedError))) =>
                                                    failIO typedError
                                                | (Left leftValue, Right (Right (Right rightValue))) =>
                                                    pureIO (leftValue, rightValue)
                                                | (Right rightValue, Left (Left defect)) =>
                                                    prelude.prim.raiseDefectIO defect
                                                | (Right rightValue, Left (Right (Left typedError))) =>
                                                    failIO typedError
                                                | (Right rightValue, Left (Right (Right leftValue))) =>
                                                    pureIO (leftValue, rightValue)
                                                | (Left leftValue, Left duplicate) =>
                                                    error "structured child sent more than one outcome"
                                                | (Right rightValue, Right duplicate) =>
                                                    error "structured child sent more than one outcome"))))))

-- | Runs an action until its millisecond deadline and returns Nothing on expiry.
-- This is the timer-specialized form of Control.Concurrent.Async's `race`, so
-- the losing action is cancelled through the same scoped-child protocol:
-- https://hackage.haskell.org/package/async/docs/Control-Concurrent-Async.html#v:race
timeout : forall e a. Int -> IO e a -> IO e (Maybe a)
timeout milliseconds action =
    bindIO
        (race
            (catchError
                (bindIO (sleep milliseconds) (fun elapsed => pureIO Nothing))
                (fun timerError =>
                    error "structured concurrency timer failed"))
            (bindIO action (fun value => pureIO (Just value))))
        (fun winner =>
            case winner of
                Left timedOut => pureIO timedOut
                | Right completed => pureIO completed)

-- | Maps an IO operation over a list with at most the requested number of
-- | operations running concurrently.
-- |
-- | Results retain input order even when operations finish out of order. The
-- | worker group is scoped: returning, failing, or interrupting the caller
-- | cancels and joins every worker. A non-positive or unsupported concurrency
-- | limit returns ConcurrentInfrastructure InvalidArgument.
mapConcurrentlyBounded : forall e a b. Int -> (a -> IO e b) -> [a] -> IO (ConcurrentError e) [b]
mapConcurrentlyBounded concurrency transform values =
    if concurrency < 1 then failIO (ConcurrentInfrastructure (HostError InvalidArgument "mapConcurrentlyBounded" ("concurrency must be between 1 and 4294967295, got " <> show concurrency)))
    else if concurrency > 4294967295 then failIO (ConcurrentInfrastructure (HostError InvalidArgument "mapConcurrentlyBounded" ("concurrency must be between 1 and 4294967295, got " <> show concurrency)))
    else
        let total = std.list.length values in
        if total == 0 then pureIO []
        else do
            jobs <- asConcurrentInfrastructure (std.channel.newBoundedChan total)
            outcomes <- asConcurrentInfrastructure (std.channel.newBoundedChan total)
            let workers = minOrd concurrency total
            withPooledWorkers
                workers
                transform
                jobs
                outcomes
                (runPooled jobs outcomes values total)

asConcurrentInfrastructure : forall e a. Task a -> IO (ConcurrentError e) a
asConcurrentInfrastructure = mapError ConcurrentInfrastructure

runPooled : forall e a b. Chan (Int, a) -> Chan (Int, (Either e b)) -> [a] -> Int -> IO (ConcurrentError e) [b]
runPooled jobs outcomes values total = do
    enqueuePooled jobs values 0
    ordered <- collectPooled outcomes total std.map.empty
    asConcurrentInfrastructure (std.channel.closeChan outcomes)
    pureIO (std.map.values ordered)

withPooledWorkers : forall e a b result. Int -> (a -> IO e b) -> Chan (Int, a) -> Chan (Int, (Either e b)) -> IO (ConcurrentError e) result -> IO (ConcurrentError e) result
withPooledWorkers count transform jobs outcomes body =
    if count == 0 then body
    else
        withThread
            (pooledWorker transform jobs outcomes)
            (fun worker => withPooledWorkers (count - 1) transform jobs outcomes body)

pooledWorker : forall e a b. (a -> IO e b) -> Chan (Int, a) -> Chan (Int, (Either e b)) -> IO (ConcurrentError e) ()
pooledWorker transform jobs outcomes =
    catchError
        (pooledWorkerLoop transform jobs outcomes)
        (fun poolError =>
            case poolError of
                ConcurrentInfrastructure (HostError ResourceVanished _ _) => pureIO ()
                | _ => failIO poolError)

pooledWorkerLoop : forall e a b. (a -> IO e b) -> Chan (Int, a) -> Chan (Int, (Either e b)) -> IO (ConcurrentError e) ()
pooledWorkerLoop transform jobs outcomes = do
    job <- asConcurrentInfrastructure (std.channel.recv jobs)
    case job of
        (index, value) => do
            outcome <- captureWorker transform value
            asConcurrentInfrastructure (std.channel.send outcomes (index, outcome))
            pooledWorkerLoop transform jobs outcomes

captureWorker : forall e a b. (a -> IO e b) -> a -> IO (ConcurrentError e) (Either e b)
captureWorker transform value =
    catchError
        (fmap Right (mapError ConcurrentWorker (transform value)))
        retainWorkerError

retainWorkerError : forall e b. ConcurrentError e -> IO (ConcurrentError e) (Either e b)
retainWorkerError (ConcurrentWorker workerError) = pureIO (Left workerError)
retainWorkerError (ConcurrentInfrastructure hostError) =
    failIO (ConcurrentInfrastructure hostError)

enqueuePooled : forall e a. Chan (Int, a) -> [a] -> Int -> IO (ConcurrentError e) ()
enqueuePooled jobs [] _ = asConcurrentInfrastructure (std.channel.closeChan jobs)
enqueuePooled jobs (value :: rest) index = do
    asConcurrentInfrastructure (std.channel.send jobs (index, value))
    enqueuePooled jobs rest (index + 1)

collectPooled : forall e a. Chan (Int, (Either e a)) -> Int -> std.map.Map Int a -> IO (ConcurrentError e) (std.map.Map Int a)
collectPooled outcomes remaining ordered =
    if remaining == 0 then pureIO ordered
    else do
        indexed <- asConcurrentInfrastructure (std.channel.recv outcomes)
        case indexed of
            (_, Left workerError) => failIO (ConcurrentWorker workerError)
            | (index, Right value) =>
                collectPooled
                    outcomes
                    (remaining - 1)
                    (std.map.insert index value ordered)
