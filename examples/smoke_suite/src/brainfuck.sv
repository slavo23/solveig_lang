import std.list
data State s a = State (s -> (a, s));

data BF
    = MoveL
    | MoveR
    | IncCell
    | DecCell
    | ReadIn
    | WriteOut
    | Loop [BF];

data BFState = BFState [Int] Int [Int] [Int] [Int];

fstPair : forall a b. (a, b) -> a;
fstPair p =
    case p of
        (x, _) => x;

sndPair : forall a b. (a, b) -> b;
sndPair p =
    case p of
        (_, y) => y;

runState : forall s a. State s a -> s -> (a, s);
runState st s =
    case st of
        State f => f s;

pureState : forall s a. a -> State s a;
pureState x =
    State (fun s => (x, s));

bindState : forall s a b. State s a -> (a -> State s b) -> State s b;
bindState st f =
    State
        (fun s =>
            case runState st s of
                (x, s1) => runState (f x) s1);

instance Functor (State s) where
    fmap f st =
        State
            (fun s =>
                case runState st s of
                    (x, s1) => (f x, s1));

instance Applicative (State s) where
    pure x = pureState x;

    apply stF stX =
        bindState stF (fun f =>
            bindState stX (fun x =>
                pureState (f x)));

instance Monad (State s) where
    bind st f = bindState st f;

getState : forall s. State s s;
getState =
    State (fun s => (s, s));

putState : forall s. s -> State s ();
putState newState =
    State (fun _ => ((), newState));

modifyState : forall s. (s -> s) -> State s ();
modifyState f =
    State (fun s => ((), f s));

initialBFState : [Int] -> BFState;
initialBFState input =
    BFState [] 0 [] input [];

bfOutput : BFState -> [Int];
bfOutput st =
    case st of
        BFState _ _ _ _ outRev => reverse outRev;

currentCell : BFState -> Int;
currentCell st =
    case st of
        BFState _ cur _ _ _ => cur;

moveRightState : State BFState ();
moveRightState =
    State
        (fun st =>
            case st of
                BFState left cur right input output =>
                    case right of
                        [] =>
                            ((), BFState (cur :: left) 0 [] input output)
                      | x :: xs =>
                            ((), BFState (cur :: left) x xs input output));

moveLeftState : State BFState ();
moveLeftState =
    State
        (fun st =>
            case st of
                BFState left cur right input output =>
                    case left of
                        [] =>
                            ((), BFState [] 0 (cur :: right) input output)
                      | x :: xs =>
                            ((), BFState xs x (cur :: right) input output));

incCellState : State BFState ();
incCellState =
    State
        (fun st =>
            case st of
                BFState left cur right input output =>
                    ((), BFState left (cur + 1) right input output));

decCellState : State BFState ();
decCellState =
    State
        (fun st =>
            case st of
                BFState left cur right input output =>
                    ((), BFState left (cur - 1) right input output));

readInState : State BFState ();
readInState =
    State
        (fun st =>
            case st of
                BFState left _ right input output =>
                    case input of
                        [] =>
                            ((), BFState left 0 right [] output)
                      | x :: xs =>
                            ((), BFState left x right xs output));

writeOutState : State BFState ();
writeOutState =
    State
        (fun st =>
            case st of
                BFState left cur right input output =>
                    ((), BFState left cur right input (cur :: output)));

execLoop : [BF] -> State BFState ();
execLoop body =
    do
        st <- getState
        if currentCell st == 0 then
            pureState ()
        else
            do
                execProgram body
                execLoop body;

execInstr : BF -> State BFState ();
execInstr instr =
    case instr of
        MoveL => moveLeftState
      | MoveR => moveRightState
      | IncCell => incCellState
      | DecCell => decCellState
      | ReadIn => readInState
      | WriteOut => writeOutState
      | Loop body => execLoop body;

execProgram : [BF] -> State BFState ();
execProgram prog =
    case prog of
        [] => pureState ()
      | i :: rest => do
            execInstr i
            execProgram rest;

runProgramState : [BF] -> [Int] -> BFState;
runProgramState prog input =
    sndPair (runState (execProgram prog) (initialBFState input));

runProgramOutput : [BF] -> [Int] -> [Int];
runProgramOutput prog input =
    bfOutput (runProgramState prog input);

echoProgram : [BF];
echoProgram =
    [ReadIn, Loop [WriteOut, ReadIn]];

addTwoInputsProgram : [BF];
addTwoInputsProgram =
    [
        ReadIn,
        MoveR,
        ReadIn,
        Loop [DecCell, MoveL, IncCell, MoveR],
        MoveL,
        WriteOut
    ];

sampleEchoOutput : [Int];
sampleEchoOutput =
    runProgramOutput echoProgram [65, 66, 67, 0];

sampleAddOutput : [Int];
sampleAddOutput =
    runProgramOutput addTwoInputsProgram [50, 97];

sampleFinalState : BFState;
sampleFinalState =
    runProgramState addTwoInputsProgram [7, 5];
