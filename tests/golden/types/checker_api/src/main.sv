data Box a = Box a
data Wrap a = Wrap a

instance Functor Box where
    fmap f box =
        case box of
            Box x => Box (f x)

instance Eq a => Eq (Wrap a) where
    (==) left right =
        case left of
            Wrap x =>
                case right of
                    Wrap y => eq x y

compose : forall a b c. (b -> c) -> (a -> b) -> a -> c
compose f g x = f (g x)

constLocal : forall a b. a -> b -> a
constLocal x y = x

applyTwice : forall a. (a -> a) -> a -> a
applyTwice f x = f (f x)

useMap : [Int] -> [Int]
useMap xs = map (fun x => x + 1) xs

floatId : Float -> Float
floatId x = x

charId : Char -> Char
charId x = x

stringId : String -> String
stringId x = x

ioIntId : IO Int -> IO Int
ioIntId x = x

sumViaFoldr : [Int] -> Int
sumViaFoldr xs = foldr (fun x => fun acc => x + acc) 0 xs

mapBox : forall a b. (a -> b) -> Box a -> Box b
mapBox f box = fmap f box

mapBoxInt : Box Int -> Box Int
mapBoxInt box = fmap (fun x => x + 1) box

sameWrap : forall a. Eq a => Wrap a -> Wrap a -> Bool
sameWrap x y = eq x y

notEqThrough : forall a. Eq a => a -> a -> Bool
notEqThrough x y = notEq x y

lessViaOrd : forall a. Ord a => a -> a -> Bool
lessViaOrd x y = less x y

sameViaOrd : forall a. Ord a => a -> a -> Bool
sameViaOrd x y = eq x y

lessInt : Bool
lessInt = less 1 2

wrapNotEqInt : Bool
wrapNotEqInt = notEq (Wrap 1) (Wrap 2)

useRank2 : (forall a. a -> a) -> Int
useRank2 f =
    let intResult = f 41 in
    let boolResult = f True in
    if boolResult then intResult + 1 else 0

useRank2Eq : (forall a. Eq a => a -> a -> Bool) -> Bool
useRank2Eq f =
    let intResult = f 1 1 in
    let boolResult = f True True in
    if intResult then boolResult else False

polyId : forall a. a -> a
polyId x = x

rank2Result : Int
rank2Result = useRank2 polyId

rank2EqResult : Bool
rank2EqResult = useRank2Eq eq

main = if lessInt then rank2Result else 0
