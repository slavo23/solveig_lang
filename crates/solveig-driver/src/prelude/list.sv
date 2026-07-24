import prelude.builtins;
import prelude.prim;

data List a = Cons a (List a) | Nil;

map : forall a b. (a -> b) -> [a] -> [b];
map f xs =
    case xs of
        [] => []
        | y :: ys => f y :: map f ys;

filter : forall a. (a -> Bool) -> [a] -> [a];
filter f xs =
    case xs of
        [] => []
        | y :: ys => if f y then y :: filter f ys else filter f ys;

foldr : forall a b. (a -> b -> b) -> b -> [a] -> b;
foldr f z xs =
    case xs of
        [] => z
        | y :: ys => f y (foldr f z ys);

foldl : forall a b. (b -> a -> b) -> b -> [a] -> b;
foldl f acc xs =
    case xs of
        [] => acc
        | y :: ys => foldl f (f acc y) ys;

concat : forall a. [[a]] -> [a]
concat xss = foldr append [] xss

append : forall a. [a] -> [a] -> [a];
append xs ys =
    case xs of
        [] => ys
        | z :: zs => z :: append zs ys;

reverse : forall a. [a] -> [a];
reverse xs =
    reverseGo xs [];

reverseGo : forall a. [a] -> [a] -> [a];
reverseGo xs acc =
    case xs of
        [] => acc
        | y :: ys => reverseGo ys (y :: acc);

range : Int -> Int -> Bool -> [Int];
range start end inclusive =
    if inclusive then
        rangeDown end start []
    else
        rangeDown (intSub end 1) start [];

rangeDown : Int -> Int -> [Int] -> [Int];
rangeDown current start acc =
    if intLt current start then acc
    else rangeDown (intSub current 1) start (current :: acc);
