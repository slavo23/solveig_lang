data Bool = True | False;
data Pair a b = MkPair a b;
data Int = Int;

even : Int -> Bool;
even n = 
    if n == 0 then True else odd (n - 1);

odd : Int -> Bool;
odd n = 
    if n == 0 then False else even (n - 1);

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