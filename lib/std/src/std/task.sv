import prelude.prim(catchDefectIO, maskIO, finallyIO, restoreMaskIO, raiseDefectIO)
import std.channel
import std.time(sleep)
import std.thread(spawn, killThread)

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
-- Peyton Jones, Gordon, and Finne, "Concurrent Haskell" (POPL 1996), section
-- 3.2, derives coordination abstractions from a private typed channel.
-- Control.Concurrent.Async documents `withAsync` as bracketed spawn and cancel:
-- https://hackage.haskell.org/package/async/docs/Control-Concurrent-Async.html
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
-- Control.Concurrent.Async's `concurrently` waits for both scoped children but
-- cancels the peer as soon as one child fails:
-- https://hackage.haskell.org/package/async/docs/Control-Concurrent-Async.html#v:concurrently
concurrently : forall e a b. IO e a -> IO e b -> IO e (Pair a b)
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
                                            case MkPair firstValue second of
                                                MkPair (Left leftValue) (Right (Left defect)) =>
                                                    prelude.prim.raiseDefectIO defect
                                                | MkPair (Left leftValue) (Right (Right (Left typedError))) =>
                                                    failIO typedError
                                                | MkPair (Left leftValue) (Right (Right (Right rightValue))) =>
                                                    pureIO (MkPair leftValue rightValue)
                                                | MkPair (Right rightValue) (Left (Left defect)) =>
                                                    prelude.prim.raiseDefectIO defect
                                                | MkPair (Right rightValue) (Left (Right (Left typedError))) =>
                                                    failIO typedError
                                                | MkPair (Right rightValue) (Left (Right (Right leftValue))) =>
                                                    pureIO (MkPair leftValue rightValue)
                                                | MkPair (Left leftValue) (Left duplicate) =>
                                                    error "structured child sent more than one outcome"
                                                | MkPair (Right rightValue) (Right duplicate) =>
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
