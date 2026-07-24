import prelude.prim

data Maybe a = Just a | Nothing;
data Either a b = Left a | Right b
data Pair a b = MkPair a b;

pureIO : forall a. a -> IO a;
pureIO value = prelude.prim.pureIO value;

bindIO : forall a b. IO a -> (a -> IO b) -> IO b;
bindIO action continuation = prelude.prim.bindIO action continuation;

printLine : String -> IO ();
printLine text = prelude.prim.printLine text;

printStr : String -> IO ();
printStr text = prelude.prim.printStr text;

readLine : IO String;
readLine = prelude.prim.readLine;

even : Int -> Bool;
even n = intEq (intMod n 2) 0;

odd : Int -> Bool;
odd n = not (even n);

not : Bool -> Bool;
not b = 
    case b of
        True => False 
        | False => True;

and : Bool -> Bool -> Bool;
and a b = 
    case MkPair a b of
          MkPair True True => True
        | MkPair False True => False
        | MkPair True False => False
        | MkPair False False => False;

or : Bool -> Bool -> Bool;
or a b = 
    case MkPair a b of
          MkPair True True => True
        | MkPair False True => True
        | MkPair True False => True
        | MkPair False False => False;
