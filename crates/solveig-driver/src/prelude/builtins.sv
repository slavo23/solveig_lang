data Maybe a = Just a | Nothing;
data Either a b = Left a | Right b
data Pair a b = MkPair a b;

even : Int -> Bool;
even n = n % 2 == 0;

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
