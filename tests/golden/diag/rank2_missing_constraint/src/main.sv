usePlainRank2 : (forall a. a -> a -> Bool) -> Bool
usePlainRank2 f = f 1 1

bad : Bool
bad = usePlainRank2 eq

main = bad
