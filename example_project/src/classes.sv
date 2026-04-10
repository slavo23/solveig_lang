class Num a where
    plus : forall a. a -> a -> a;
#end;

instance Num Int where
    plus x y = x + y;
#end;