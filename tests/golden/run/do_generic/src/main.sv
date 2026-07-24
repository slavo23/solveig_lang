data State s a = State (s -> Pair a s)

runState : forall s a. State s a -> s -> Pair a s
runState action state =
    case action of
        State run => run state

statePure : forall s a. a -> State s a
statePure value = State (fun state => MkPair value state)

stateBind : forall s a b. State s a -> (a -> State s b) -> State s b
stateBind action continuation =
    State (fun state =>
        case runState action state of
            MkPair value nextState => runState (continuation value) nextState)

instance Functor (State s) where
    fmap f action =
        State (fun state =>
            case runState action state of
                MkPair value nextState => MkPair (f value) nextState)

instance Applicative (State s) where
    pure value = statePure value

    apply actionF actionX =
        State (fun state =>
            case runState actionF state of
                MkPair f stateAfterF =>
                    case runState actionX stateAfterF of
                        MkPair x stateAfterX => MkPair (f x) stateAfterX)

instance Monad (State s) where
    bind action continuation = stateBind action continuation

tick : State Int Int
tick =
    State (fun state => MkPair state (state + 1))

maybeResult : Maybe Int
maybeResult = do
    x <- Just 2
    y <- if x == 2 then Just 40 else Nothing
    let total = x + y
    pure total

stateProgram : State Int Int
stateProgram = do
    x <- tick
    _ <- tick
    y <- tick
    pure (x + y)

stateResult : Pair Int Int
stateResult = runState stateProgram 10

main : Pair (Maybe Int) (Pair Int Int)
main = MkPair maybeResult stateResult
