data Ordering3 = Lt3 | Eq3 | Gt3

data Wrap a = Wrap a

class Eq a => OrdLike a where
    cmpLike : a -> a -> Ordering3
    lessLike : a -> a -> Bool

class OrdLike a => BoundedLike a where
    lowLike : a
    highLike : a
    clampLike : a -> a

class Functor f => Pointed f where
    pureLike : forall a. a -> f a

class Pointed m => BindLike m where
    bindLike : forall a b. m a -> (a -> m b) -> m b


instance OrdLike Int where
    cmpLike x y =
        if x < y then Lt3
        else if x > y then Gt3
        else Eq3

    lessLike x y =
        if x < y then True else eq x y


instance BoundedLike Int where
    lowLike = 0

    highLike = 100

    clampLike x =
        if lessLike x 0 then 0
        else if lessLike 100 x then 100
        else x


instance Eq a => Eq (Wrap a) where
    eq left right =
        case left of
            Wrap x =>
                case right of
                    Wrap y => eq x y


instance OrdLike a => OrdLike (Wrap a) where
    cmpLike left right =
        case left of
            Wrap x =>
                case right of
                    Wrap y => cmpLike x y

    lessLike left right =
        case left of
            Wrap x =>
                case right of
                    Wrap y => lessLike x y


instance BoundedLike a => BoundedLike (Wrap a) where
    lowLike = Wrap lowLike

    highLike = Wrap highLike

    clampLike wrapped =
        case wrapped of
            Wrap x => Wrap (clampLike x)


instance OrdLike a => OrdLike (Maybe a) where
    cmpLike left right =
        case left of
            Nothing =>
                case right of
                    Nothing => Eq3
                    | Just _ => Lt3
            | Just x =>
                case right of
                    Nothing => Gt3
                    | Just y => cmpLike x y

    lessLike left right =
        case left of
            Nothing => True
            | Just x =>
                case right of
                    Nothing => False
                    | Just y => lessLike x y


instance BoundedLike a => BoundedLike (Maybe a) where
    lowLike = Nothing

    highLike = Just highLike

    clampLike mx =
        case mx of
            Nothing => Nothing
            | Just x => Just (clampLike x)


instance Pointed Maybe where
    pureLike x = Just x


instance BindLike Maybe where
    bindLike mx f =
        case mx of
            Nothing => Nothing
            | Just x => f x


instance Pointed List where
    pureLike x = [x]


instance BindLike List where
    bindLike xs f = concat (fmap f xs)


sameFromOrd : forall a. OrdLike a => a -> a -> Bool
sameFromOrd x y = eq x y

sameFromBounded : forall a. BoundedLike a => a -> a -> Bool
sameFromBounded x y = eq x y

lessFromBounded : forall a. BoundedLike a => a -> a -> Bool
lessFromBounded x y = lessLike x y

cmpFromBounded : forall a. BoundedLike a => a -> a -> Ordering3
cmpFromBounded x y = cmpLike x y

eqAndLess : forall a. (Eq a, OrdLike a) => a -> a -> Bool
eqAndLess x y =
    if eq x y then True else lessLike x y

pureFromBind : forall m a. BindLike m => a -> m a
pureFromBind x = pureLike x

mapFromBind : forall m a b. BindLike m => (a -> b) -> m a -> m b
mapFromBind f mx = fmap f mx

bindThenPure : forall m a. BindLike m => m a -> m a
bindThenPure mx = bindLike mx (fun x => pureLike x)


ordSuperclassIntEq : Bool
ordSuperclassIntEq = sameFromOrd 10 10

boundedTransitiveIntEq : Bool
boundedTransitiveIntEq = sameFromBounded 25 25

boundedTransitiveIntLess : Bool
boundedTransitiveIntLess = lessFromBounded 10 20

boundedTransitiveIntCmp : Ordering3
boundedTransitiveIntCmp = cmpFromBounded 10 20

multipleConstraintUse : Bool
multipleConstraintUse = eqAndLess 5 6


wrappedOrdEq : Bool
wrappedOrdEq = sameFromOrd (Wrap 7) (Wrap 7)

wrappedBoundedEq : Bool
wrappedBoundedEq = sameFromBounded (Wrap 8) (Wrap 8)

wrappedLess : Bool
wrappedLess = lessFromBounded (Wrap 1) (Wrap 2)

wrappedClamp : Wrap Int
wrappedClamp = clampLike (Wrap 150)


maybeOrdEq : Bool
maybeOrdEq = sameFromOrd (Just 11) (Just 11)

maybeNestedOrdEq : Bool
maybeNestedOrdEq = sameFromOrd (Just (Just 11)) (Just (Just 11))

maybeBoundedEq : Bool
maybeBoundedEq = sameFromBounded (Just 12) (Just 12)

maybeWrappedBoundedEq : Bool
maybeWrappedBoundedEq = sameFromBounded (Just (Wrap 12)) (Just (Wrap 12))

maybeClampLow : Maybe Int
maybeClampLow = clampLike (Just (-10))

maybeClampHigh : Maybe Int
maybeClampHigh = clampLike (Just 1000)

maybeClampNested : Maybe (Wrap Int)
maybeClampNested = clampLike (Just (Wrap 1000))


pointedMaybe : Maybe Int
pointedMaybe = pureLike 42

bindMaybe : Maybe Int
bindMaybe = bindLike (Just 10) (fun x => Just (x + 1))

pureFromBindMaybe : Maybe Int
pureFromBindMaybe = pureFromBind 99

mapFromBindMaybe : Maybe Int
mapFromBindMaybe = mapFromBind (fun x => x + 1) (Just 20)

bindThenPureMaybe : Maybe Int
bindThenPureMaybe = bindThenPure (Just 123)


pointedList : [Int]
pointedList = pureLike 42

bindList : [Int]
bindList = bindLike [1, 2, 3] (fun x => [x, x + 10])

pureFromBindList : [Int]
pureFromBindList = pureFromBind 77

mapFromBindList : [Int]
mapFromBindList = mapFromBind (fun x => x + 1) [10, 20, 30]

bindThenPureList : [Int]
bindThenPureList = bindThenPure [4, 5, 6]


eqMaybeFromBindResult : Bool
eqMaybeFromBindResult = eq bindMaybe (Just 11)

eqListFromBindResult : Bool
eqListFromBindResult = eq mapFromBindList [11, 21, 31]

eqWrappedMaybeBounded : Bool
eqWrappedMaybeBounded =
    sameFromBounded (Wrap (Just 1)) (Wrap (Just 1))


typeclassSuperclassSuccessMain : Int
typeclassSuperclassSuccessMain =
    let a = ordSuperclassIntEq in
    let b = boundedTransitiveIntEq in
    let c = boundedTransitiveIntLess in
    let d = boundedTransitiveIntCmp in
    let e = multipleConstraintUse in
    let f = wrappedOrdEq in
    let g = wrappedBoundedEq in
    let h = wrappedLess in
    let i = wrappedClamp in
    let j = maybeOrdEq in
    let k = maybeNestedOrdEq in
    let l = maybeBoundedEq in
    let m = maybeWrappedBoundedEq in
    let n = maybeClampLow in
    let o = maybeClampHigh in
    let p = maybeClampNested in
    let q = pointedMaybe in
    let r = bindMaybe in
    let s = pureFromBindMaybe in
    let t = mapFromBindMaybe in
    let u = bindThenPureMaybe in
    let v = pointedList in
    let w = bindList in
    let x = pureFromBindList in
    let y = mapFromBindList in
    let z = bindThenPureList in
    let aa = eqMaybeFromBindResult in
    let ab = eqListFromBindResult in
    let ac = eqWrappedMaybeBounded in
    1