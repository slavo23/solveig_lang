prepend : forall a. a -> [a] -> [a]
prepend = (::)

increment : Int -> Int
increment = (+) 1

renderIncremented : Int -> String
renderIncremented = show . increment

maybeIncremented : Maybe Int
maybeIncremented = increment <$> Just 4

maybeApplied : Maybe Int
maybeApplied = Just increment <*> Just 9

maybeBound : Maybe Int
maybeBound = Just 20 >>= (fun value => Just (value + 2))

nan : Float
nan = 0.0 / 0.0

main : Task ()
main = do
    printLine $ show (prepend 1 [2, 3])
    printLine (renderIncremented 41)
    printLine (show maybeIncremented)
    printLine (show maybeApplied)
    printLine (show maybeBound)
    printLine ("operators" <> " are terms")
    printLine (show (partialCompare nan 1.0))
    printLine (show (totalCompareFloat nan nan))
