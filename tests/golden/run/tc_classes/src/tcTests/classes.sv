data Box a = Box a
data BoxInt = BoxInt Int

instance Functor Box where
    fmap f box =
        case box of
            Box x => Box (f x)

instance Eq BoxInt where
    eq left right =
        case left of
            BoxInt x =>
                case right of
                    BoxInt y => eq x y

sameTwice : forall a. Eq a => a -> a -> Bool
sameTwice x y = eq x y

chooseIfSame : forall a. Eq a => a -> a -> a -> a
chooseIfSame x y fallback =
    if eq x y then x else fallback

mapId : forall f a. Functor f => f a -> f a
mapId xs = fmap (fun x => x) xs

mapConstInt : forall f a. Functor f => f a -> f Int
mapConstInt xs = fmap (fun _ => 100) xs

-- Imported Eq instances.
eqImportedInt : Bool
eqImportedInt = eq 10 10

eqImportedBool : Bool
eqImportedBool = eq True False

eqLocalBoxIntTrue : Bool
eqLocalBoxIntTrue = eq (BoxInt 5) (BoxInt 5)

eqLocalBoxIntViaConstraint : Bool
eqLocalBoxIntViaConstraint = sameTwice (BoxInt 7) (BoxInt 7)

chooseLocalBoxInt : BoxInt
chooseLocalBoxInt = chooseIfSame (BoxInt 1) (BoxInt 1) (BoxInt 999)

mapMaybeDirect : Maybe Int
mapMaybeDirect = fmap (fun x => x + 1) (Just 10)

mapMaybeViaConstraint : Maybe Int
mapMaybeViaConstraint = mapId (Just 42)

mapMaybeConst : Maybe Int
mapMaybeConst = mapConstInt (Just True)

nothingInt : Maybe Int
nothingInt = Nothing

mapNothingViaConstraint : Maybe Int
mapNothingViaConstraint = mapConstInt nothingInt


mapListDirect : [Int]
mapListDirect = fmap (fun x => x + 1) [1, 2, 3, 4, 5]

mapListViaConstraint : [Int]
mapListViaConstraint = mapId [10, 20, 30]

mapListConst : [Int]
mapListConst = mapConstInt [True, False, True]


mapBoxDirect : Box Int
mapBoxDirect = fmap (fun x => x + 1) (Box 41)

mapBoxViaConstraint : Box Int
mapBoxViaConstraint = mapId (Box 123)

mapBoxConst : Box Int
mapBoxConst = mapConstInt (Box True)

nestedBoxMaybe : Box (Maybe Int)
nestedBoxMaybe =
    fmap
        (fun mx => fmap (fun x => x + 1) mx)
        (Box (Just 10))


nestedEqUse : Bool
nestedEqUse =
    let a = BoxInt 100 in
    let b = BoxInt 100 in
    sameTwice a b

eqMaybeIntExample : Bool
eqMaybeIntExample = eq (Just 1) (Just 1)

eqMaybeBoolExample : Bool
eqMaybeBoolExample = eq (Just True) (Just True)

eqNestedMaybeIntExample : Bool
eqNestedMaybeIntExample = eq (Just (Just 1)) (Just (Just 1))

eqListIntExample : Bool
eqListIntExample = eq [1, 2, 3] [1, 2, 3]

eqListMaybeIntExample : Bool
eqListMaybeIntExample = eq [Just 1, Nothing] [Just 1, Nothing]

eqListInt : Bool
eqListInt = eq [1, 2, 3] [1, 2, 3]

eqListMaybeInt : Bool
eqListMaybeInt = eq [Just 1, Nothing] [Just 1, Nothing]

eqMaybeListInt : Bool
eqMaybeListInt = eq (Just [1, 2]) (Just [1, 2])


advancedTypeclassRegressionMain : Int
advancedTypeclassRegressionMain =
    let a = eqImportedInt in
    let b = eqImportedBool in
    let c = eqLocalBoxIntTrue in
    let d = eqLocalBoxIntViaConstraint in
    let e = chooseLocalBoxInt in
    let f = mapMaybeDirect in
    let g = mapMaybeViaConstraint in
    let h = mapMaybeConst in
    let i = mapNothingViaConstraint in
    let j = mapListDirect in
    let k = mapListViaConstraint in
    let l = mapListConst in
    let m = mapBoxDirect in
    let n = mapBoxViaConstraint in
    let o = mapBoxConst in
    let p = nestedBoxMaybe in
    let q = nestedEqUse in
    let q2 = eqMaybeIntExample in 
    let q3 = eqMaybeBoolExample in 
    let q4 = eqNestedMaybeIntExample in
    let q5 = eqListIntExample in 
    let q6 = eqListMaybeIntExample in
    let q7 = [eqListInt, eqListMaybeInt, eqMaybeListInt] in
    123
