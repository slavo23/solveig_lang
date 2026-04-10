import list(bigList);

sumEvenBigList =
    let evenList = filter even bigList in
    foldl (fun acc x => acc + x) 0 evenList;

twoArg x y = x;

class Num a where
    plus : forall a. a -> a -> a;
    minus : forall a. a -> a -> a;
#end;

instance Num Int where
    plus x y = x + y;
    minus x y = x - y;
#end;


main = twoArg 1 2;