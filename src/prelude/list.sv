import prelude.builtins;

data List a = Cons a (List a) | Nil;

map : forall a b. (a -> b) -> List a -> List b;
map f xs = 
    case xs of
        [] => []
        | y :: ys => (f y) :: map f ys;

filter : forall a. (a -> Bool) -> [a] -> [a];
filter f xs = 
    case xs of
        [] => []
        | y :: ys => if (f y) then y :: filter f ys else filter f ys;

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

append : forall a. [a] -> [a] -> [a];
append xs ys = 
    case xs of
        [] => ys
        | z :: zs => z :: append zs ys;

range : Int -> Int -> Bool -> [Int];
range start end inclusive =
    if start > end then []
    else if start == end then
        if inclusive then [start] else []
    else
        start :: range (start + 1) end inclusive;