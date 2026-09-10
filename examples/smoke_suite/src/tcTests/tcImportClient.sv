import tcTests.tcImportLib(BoundedLike)

class Eq a => NeedsPreludeEq a where
    keepEq : a -> a

instance BoundedLike a => NeedsPreludeEq a where
    keepEq x = x

sameViaNeed : forall a. NeedsPreludeEq a => a -> a -> Bool
sameViaNeed x y = eq x y

methodImportedByClassSelection : Int
methodImportedByClassSelection = clampLike 200

transitiveImportedSuperclassOk : Bool
transitiveImportedSuperclassOk = sameViaNeed 10 10

importSmokeMain : Int

importSmokeMain =
    let a = methodImportedByClassSelection in let b = transitiveImportedSuperclassOk in 123
