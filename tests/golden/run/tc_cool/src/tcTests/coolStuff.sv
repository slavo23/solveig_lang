data Tree a = EmptyTree | TreeNode (Tree a) a (Tree a)
data Log a = MkLog [Int] a

instance Eq a => Eq (Tree a) where
    (==) left right =
        case left of
            EmptyTree =>
                case right of
                    EmptyTree => True
                    | TreeNode _ _ _ => False
            | TreeNode leftL leftX leftR =>
                case right of
                    EmptyTree => False
                    | TreeNode rightL rightX rightR =>
                        if eq leftX rightX then
                            if eq leftL rightL then eq leftR rightR else False
                        else False


instance Functor Tree where
    fmap f tree =
        case tree of
            EmptyTree => EmptyTree
            | TreeNode left x right =>
                TreeNode (fmap f left) (f x) (fmap f right)


instance Eq a => Eq (Log a) where
    (==) left right =
        case left of
            MkLog leftNotes leftValue =>
                case right of
                    MkLog rightNotes rightValue =>
                        if eq leftNotes rightNotes then eq leftValue rightValue else False


instance Functor Log where
    fmap f logged =
        case logged of
            MkLog notes x => MkLog notes (f x)


instance Applicative Log where
    pure x = MkLog [] x

    apply loggedF loggedX =
        case loggedF of
            MkLog notesF f =>
                case loggedX of
                    MkLog notesX x =>
                        MkLog (append notesF notesX) (f x)


instance Monad Log where
    bind logged f =
        case logged of
            MkLog notes x =>
                case f x of
                    MkLog moreNotes y =>
                        MkLog (append notes moreNotes) y


treeInsert : forall a. Ord a => a -> Tree a -> Tree a
treeInsert x tree =
    case tree of
        EmptyTree => TreeNode EmptyTree x EmptyTree
        | TreeNode left y right =>
            if eq x y then
                TreeNode left y right
            else if less x y then
                TreeNode (treeInsert x left) y right
            else
                TreeNode left y (treeInsert x right)


treeMember : forall a. Ord a => a -> Tree a -> Bool
treeMember x tree =
    case tree of
        EmptyTree => False
        | TreeNode left y right =>
            if eq x y then True
            else if less x y then treeMember x left
            else treeMember x right


treeFromList : forall a. Ord a => [a] -> Tree a
treeFromList xs =
    foldl (fun tree => fun x => treeInsert x tree) EmptyTree xs


treeToList : forall a. Tree a -> [a]
treeToList tree =
    case tree of
        EmptyTree => []
        | TreeNode left x right =>
            append (treeToList left) (x :: treeToList right)


treeSortUnique : forall a. Ord a => [a] -> [a]
treeSortUnique xs = treeToList (treeFromList xs)


treeHeight : forall a. Tree a -> Int
treeHeight tree =
    case tree of
        EmptyTree => 0
        | TreeNode left x right =>
            1 + maxOrd (treeHeight left) (treeHeight right)


treeMapM : forall m a b. Monad m => (a -> m b) -> Tree a -> m (Tree b)
treeMapM f tree =
    case tree of
        EmptyTree => pure EmptyTree
        | TreeNode left x right =>
            bind (treeMapM f left) (fun newLeft =>
                bind (f x) (fun newX =>
                    bind (treeMapM f right) (fun newRight =>
                        pure (TreeNode newLeft newX newRight))))


validateNonNegative : Int -> Maybe Int
validateNonNegative x =
    if less x 0 then Nothing else Just x


validateSmall : Int -> Maybe Int
validateSmall x =
    if greater x 100 then Nothing else Just x


validateTree : Tree Int -> Maybe (Tree Int)
validateTree tree =
    treeMapM validateNonNegative tree


validateSmallTree : Tree Int -> Maybe (Tree Int)
validateSmallTree tree =
    treeMapM validateSmall tree


safeHalf : Int -> Maybe Int
safeHalf x =
    if even x then Just (div x 2) else Nothing


safeIncrementBelowLimit : Int -> Maybe Int
safeIncrementBelowLimit x =
    if less x 50 then Just (x + 1) else Nothing


safePipeline : Int -> Maybe Int
safePipeline = composeK safeHalf safeIncrementBelowLimit


choosePair : [Int] -> [Int] -> [Pair Int Int]
choosePair xs ys =
    bind xs (fun x =>
        bind ys (fun y =>
            pure (MkPair x y)))


orderedPairs : [Int] -> [Pair Int Int]
orderedPairs xs =
    bind xs (fun x =>
        bind xs (fun y =>
            if less x y then [MkPair x y] else []))


pythagoreanTiny : [Pair Int Int]
pythagoreanTiny =
    bind [3, 4, 5, 6] (fun x =>
        bind [3, 4, 5, 6] (fun y =>
            if eq (x * x + y * y) 25 then [MkPair x y] else []))


allBooleanAssignments3 : [[Bool]]
allBooleanAssignments3 =
    sequence [[True, False], [True, False], [True, False]]


allSubsets3 : [[Int]]
allSubsets3 =
    filterM (fun x => [True, False]) [1, 2, 3]


applicativeGridSums : [Int]
applicativeGridSums =
    liftA2 (fun x => fun y => x + y) [1, 2, 3] [10, 20]


applicativeMaybeCombine : Maybe Int
applicativeMaybeCombine =
    liftA2 (fun x => fun y => x * y) (Just 6) (Just 7)


logStep : Int -> Log Int
logStep x =
    MkLog [x] (x + 1)


logDouble : Int -> Log Int
logDouble x =
    MkLog [x * 10] (x * 2)


loggedProgram : Log Int
loggedProgram =
    bind (logStep 3) (fun x =>
        bind (logDouble x) (fun y =>
            pure (y + 1)))


loggedTree : Log (Tree Int)
loggedTree =
    treeMapM logStep (treeFromList [3, 1, 4])


sumListsMonoid : [Int]
sumListsMonoid =
    mconcat [[1, 2], [3], [], [4, 5]]


pairMonoidAggregation : Pair [Int] [Int]
pairMonoidAggregation =
    mconcat [
        MkPair [1] [10],
        MkPair [2, 3] [],
        MkPair [] [20, 30]
    ]


clampedInts : [Int]
clampedInts =
    map (clampOrd 0 10) [-5, 0, 3, 10, 999]


sampleTree : Tree Int
sampleTree =
    treeFromList [5, 3, 9, 1, 4, 7, 9, 3]


sampleTreeSorted : [Int]
sampleTreeSorted =
    treeSortUnique [5, 3, 9, 1, 4, 7, 9, 3]


sampleTreeMapped : Tree Int
sampleTreeMapped =
    fmap (fun x => x + 1) sampleTree


sampleTreeValidated : Maybe (Tree Int)
sampleTreeValidated =
    validateTree sampleTree


badTreeValidated : Maybe (Tree Int)
badTreeValidated =
    validateTree (treeFromList [1, -2, 3])


smallTreeValidated : Maybe (Tree Int)
smallTreeValidated =
    validateSmallTree sampleTree


tooLargeTreeValidated : Maybe (Tree Int)
tooLargeTreeValidated =
    validateSmallTree (treeFromList [1, 200, 3])


safePipelineOk : Maybe Int
safePipelineOk =
    safePipeline 20


safePipelineRejectedByEven : Maybe Int
safePipelineRejectedByEven =
    safePipeline 21


safePipelineRejectedByLimit : Maybe Int
safePipelineRejectedByLimit =
    safePipeline 200


checkTreeSort : Bool
checkTreeSort =
    eq sampleTreeSorted [1, 3, 4, 5, 7, 9]


checkTreeMemberPresent : Bool
checkTreeMemberPresent =
    treeMember 7 sampleTree


checkTreeMemberAbsent : Bool
checkTreeMemberAbsent =
    not (treeMember 8 sampleTree)


checkTreeHeight : Bool
checkTreeHeight =
    eq (treeHeight sampleTree) 4


checkTreeFunctor : Bool
checkTreeFunctor =
    eq (treeToList sampleTreeMapped) [2, 4, 5, 6, 8, 10]


checkValidateTree : Bool
checkValidateTree =
    eq sampleTreeValidated (Just sampleTree)


checkValidateBadTree : Bool
checkValidateBadTree =
    eq badTreeValidated Nothing


checkValidateSmallTree : Bool
checkValidateSmallTree =
    eq smallTreeValidated (Just sampleTree)


checkValidateTooLargeTree : Bool
checkValidateTooLargeTree =
    eq tooLargeTreeValidated Nothing


checkSafePipelineOk : Bool
checkSafePipelineOk =
    eq safePipelineOk (Just 11)


checkSafePipelineRejectedByEven : Bool
checkSafePipelineRejectedByEven =
    eq safePipelineRejectedByEven Nothing


checkSafePipelineRejectedByLimit : Bool
checkSafePipelineRejectedByLimit =
    eq safePipelineRejectedByLimit Nothing


checkChoosePair : Bool
checkChoosePair =
    eq (choosePair [1, 2] [10, 20])
       [MkPair 1 10, MkPair 1 20, MkPair 2 10, MkPair 2 20]


checkOrderedPairs : Bool
checkOrderedPairs =
    eq (orderedPairs [1, 2, 3])
       [MkPair 1 2, MkPair 1 3, MkPair 2 3]


checkPythagoreanTiny : Bool
checkPythagoreanTiny =
    eq pythagoreanTiny [MkPair 3 4, MkPair 4 3]


checkBooleanAssignments : Bool
checkBooleanAssignments =
    eq allBooleanAssignments3 [
        [True, True, True],
        [True, True, False],
        [True, False, True],
        [True, False, False],
        [False, True, True],
        [False, True, False],
        [False, False, True],
        [False, False, False]
    ]


checkAllSubsets3 : Bool
checkAllSubsets3 =
    eq allSubsets3 [
        [1, 2, 3],
        [1, 2],
        [1, 3],
        [1],
        [2, 3],
        [2],
        [3],
        []
    ]


checkApplicativeGridSums : Bool
checkApplicativeGridSums =
    eq applicativeGridSums [11, 21, 12, 22, 13, 23]


checkApplicativeMaybeCombine : Bool
checkApplicativeMaybeCombine =
    eq applicativeMaybeCombine (Just 42)


checkLoggedProgram : Bool
checkLoggedProgram =
    eq loggedProgram (MkLog [3, 40] 9)


checkLoggedTree : Bool
checkLoggedTree =
    eq loggedTree (MkLog [1, 3, 4, 5, 7, 9] (treeFromList [2, 4, 5, 6, 8, 10]))


checkSumListsMonoid : Bool
checkSumListsMonoid =
    eq sumListsMonoid [1, 2, 3, 4, 5]


checkPairMonoidAggregation : Bool
checkPairMonoidAggregation =
    eq pairMonoidAggregation (MkPair [1, 2, 3] [10, 20, 30])


checkClampedInts : Bool
checkClampedInts =
    eq clampedInts [0, 0, 3, 10, 10]


coolTypeclassExamplesMain : Int
coolTypeclassExamplesMain =
    let a = checkTreeSort in
    let b = checkTreeMemberPresent in
    let c = checkTreeMemberAbsent in
    let d = checkTreeHeight in
    let e = checkTreeFunctor in
    let f = checkValidateTree in
    let g = checkValidateBadTree in
    let h = checkValidateSmallTree in
    let i = checkValidateTooLargeTree in
    let j = checkSafePipelineOk in
    let k = checkSafePipelineRejectedByEven in
    let l = checkSafePipelineRejectedByLimit in
    let m = checkChoosePair in
    let n = checkOrderedPairs in
    let o = checkPythagoreanTiny in
    let p = checkBooleanAssignments in
    let q = checkAllSubsets3 in
    let r = checkApplicativeGridSums in
    let s = checkApplicativeMaybeCombine in
    let t = checkLoggedProgram in
    let u = checkLoggedTree in
    let v = checkSumListsMonoid in
    let w = checkPairMonoidAggregation in
    let x = checkClampedInts in
    4242
