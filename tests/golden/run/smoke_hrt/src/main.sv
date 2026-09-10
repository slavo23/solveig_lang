useRank2 : (forall a. a -> a) -> Int
useRank2 f =
    let intResult = f 41 in let boolResult = f True in if boolResult then intResult + 1 else 0

polyId : forall a. a -> a
polyId x = x

main = useRank2 polyId
