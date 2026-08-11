import prelude.prim

-- | A value that may be present or absent.
data Maybe a =
    -- | Wraps a present value.
    Just a
  | -- | Represents the absence of a value.
    Nothing;

-- | A value that can hold one of two alternatives.
data Either a b =
    -- | Holds a value from the left side.
    Left a
  | -- | Holds a value from the right side.
    Right b

-- | A pair of two values.
data Pair a b =
    -- | Builds a pair from its first and second values.
    MkPair a b;

-- | An error type with no possible values.
data Never

-- | Failures raised by the built-in terminal and file operations.
data IOError =
    -- | Writing to standard output failed.
    StdoutError String
  | -- | Reading from standard input failed.
    StdinError String
  | -- | Reading a UTF-8 text file failed, with path and host message.
    ReadFileError String String
  | -- | Replacing a file failed, with path and host message.
    WriteFileError String String
  | -- | Appending to a file failed, with path and host message.
    AppendFileError String String

-- | A defect raised by pure evaluation or a managed runtime limit.
data Defect =
    -- | An explicit unrecoverable condition raised by user code.
    Error String
  | -- | Integer division or modulo used a zero divisor.
    DivisionByZero
  | -- | A process exceeded the managed VM stack limit.
    StackExhausted
  | -- | The runtime exceeded the managed heap-object limit.
    HeapExhausted

-- | An IO action using the built-in error vocabulary.
type Task a = IO IOError a;

-- | Creates an IO action that immediately returns a value.
pureIO : forall e a. a -> IO e a;
pureIO value = prelude.prim.pureIO value;

-- | Sequences an IO action and passes its result to a continuation.
bindIO : forall e a b. IO e a -> (a -> IO e b) -> IO e b;
bindIO action continuation = prelude.prim.bindIO action continuation;

-- | Creates an IO action that fails with a typed error.
failIO : forall e a. e -> IO e a;
failIO error = prelude.prim.failIO error;

-- | Raises a defect from pure code.
error : forall a. String -> a;
error message = prelude.prim.error message;

-- | Handles a typed error and may change the action's error type.
catchError : forall e f a. IO e a -> (e -> IO f a) -> IO f a;
catchError action handler = prelude.prim.catchIO action handler;

-- | Handles defects raised while executing an IO action's installed scope.
catchDefect : forall e a. IO e a -> (Defect -> IO e a) -> IO e a;
catchDefect action handler = prelude.prim.catchDefectIO action handler;

-- | Transforms the typed error of an IO action.
mapError : forall e f a. (e -> f) -> IO e a -> IO f a;
mapError transform action =
    catchError action (fun error => failIO (transform error));

-- | Uses a fallback action when the first action fails.
orElse : forall e f a. IO e a -> IO f a -> IO f a;
orElse action fallback = catchError action (fun error => fallback);

-- | Runs cleanup after an action succeeds or raises a typed error.
finally : forall e a b. IO e a -> IO e b -> IO e a;
finally action cleanup = prelude.prim.finallyIO action cleanup;

-- | Acquires a resource and releases it after its use action finishes.
bracket : forall e resource cleanup a. IO e resource -> (resource -> IO e cleanup) -> (resource -> IO e a) -> IO e a;
bracket acquire release use =
    bindIO acquire (fun resource => finally (use resource) (release resource));

-- | Writes text followed by a newline.
printLine : String -> Task ();
printLine text = prelude.prim.printLine text;

-- | Writes text without adding a newline.
printStr : String -> Task ();
printStr text = prelude.prim.printStr text;

-- | Reads one line of input.
readLine : Task String;
readLine = prelude.prim.readLine;

-- | Reads a file as text.
readFile : String -> Task String;
readFile path = prelude.prim.readFile path;

-- | Writes text to a file, replacing its previous contents.
writeFile : String -> String -> Task ();
writeFile path contents = prelude.prim.writeFile path contents;

-- | Appends text to the end of a file.
appendFile : String -> String -> Task ();
appendFile path contents = prelude.prim.appendFile path contents;

-- | Starts an IO action in a new lightweight thread.
spawn : forall childError e. IO childError () -> IO e ThreadId;
spawn action = prelude.prim.spawn action;

-- | Concatenates two strings.
appendString : String -> String -> String;
appendString left right = prelude.prim.stringAppend left right;

-- | Checks whether an integer is even.
even : Int -> Bool;
even n = intEq (intMod n 2) 0;

-- | Checks whether an integer is odd.
odd : Int -> Bool;
odd n = not (even n);

-- | Flips a boolean value.
not : Bool -> Bool;
not True = False;
not False = True;

-- | Returns True only when both inputs are True.
and : Bool -> Bool -> Bool;
and True True = True;
and _ _ = False;

-- | Returns True when at least one input is True.
or : Bool -> Bool -> Bool;
or False False = False;
or _ _ = True;
