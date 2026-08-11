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

-- | Creates an IO action that immediately returns a value.
pureIO : forall e a. a -> IO e a;
pureIO value = prelude.prim.pureIO value;

-- | Sequences an IO action and passes its result to a continuation.
bindIO : forall e a b. IO e a -> (a -> IO e b) -> IO e b;
bindIO action continuation = prelude.prim.bindIO action continuation;

-- | Writes text followed by a newline.
printLine : forall e. String -> IO e ();
printLine text = prelude.prim.printLine text;

-- | Writes text without adding a newline.
printStr : forall e. String -> IO e ();
printStr text = prelude.prim.printStr text;

-- | Reads one line of input.
readLine : forall e. IO e String;
readLine = prelude.prim.readLine;

-- | Reads a file as text.
readFile : forall e. String -> IO e String;
readFile path = prelude.prim.readFile path;

-- | Writes text to a file, replacing its previous contents.
writeFile : forall e. String -> String -> IO e ();
writeFile path contents = prelude.prim.writeFile path contents;

-- | Appends text to the end of a file.
appendFile : forall e. String -> String -> IO e ();
appendFile path contents = prelude.prim.appendFile path contents;

-- | Starts an IO action in a new lightweight thread.
spawn : forall e. IO e () -> IO e ThreadId;
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
