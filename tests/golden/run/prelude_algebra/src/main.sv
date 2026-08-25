check : String -> Bool -> Task ()
check label condition =
    if condition then printLine ("ok: " <> label) else error ("law failed: " <> label)

buildRange : Int -> [Int] -> [Int]
buildRange 0 values = values
buildRange remaining values = buildRange (remaining - 1) (remaining :: values)

count : forall t a. Foldable t => t a -> Int
count values = foldl (fun total _ => total + 1) 0 values

hashCoherent : forall a. Hashable a => a -> a -> Bool
hashCoherent left right = if left == right then hash left == hash right else True

maybeStep : Int -> Maybe Int
maybeStep value = if value < 4 then Just (value + 10) else Nothing

maybeToEither : Maybe [Int] -> Either String [Int]
maybeToEither Nothing = Left "missing"
maybeToEither (Just values) = Right values

elementToEither : Maybe Int -> Either String Int
elementToEither Nothing = Left "missing"
elementToEither (Just value) = Right value

visit : Int -> Task Int
visit value = do
    printLine ("visit " <> show value)
    pureIO (value * 2)

data MaybeEither a = MaybeEither (Maybe (Either String a))

instance Eq a => Eq (MaybeEither a) where
    (==) (MaybeEither left) (MaybeEither right) = left == right

instance Functor MaybeEither where
    fmap transform (MaybeEither value) = MaybeEither (fmap (fmap transform) value)

instance Applicative MaybeEither where
    pure value = MaybeEither (Just (Right value))
    apply (MaybeEither Nothing) _ = MaybeEither Nothing
    apply _ (MaybeEither Nothing) = MaybeEither Nothing
    apply (MaybeEither (Just (Left error))) _ = MaybeEither (Just (Left error))
    apply (MaybeEither (Just (Right transform))) (MaybeEither (Just (Left error))) =
        MaybeEither (Just (Left error))
    apply (MaybeEither (Just (Right transform))) (MaybeEither (Just (Right value))) =
        MaybeEither (Just (Right (transform value)))

toRight : Int -> Either String Int
toRight value = Right (value * 2)

compositionStep : Int -> MaybeEither Int
compositionStep value = MaybeEither (fmap toRight (maybeStep value))

compositionRight : [Int] -> MaybeEither [Int]
compositionRight values = MaybeEither (fmap (traverse toRight) (traverse maybeStep values))

foldableTupleLaws : Bool
foldableTupleLaws =
    and
        (foldl (+) 0 ("a", 2) == 2)
        (and
            (foldl (+) 0 ("a", False, 3) == 3)
            (and
                (foldl (+) 0 ("a", False, 'x', 4) == 4)
                (and
                    (foldl (+) 0 ("a", False, 'x', (), 5) == 5)
                    (and
                        (foldl (+) 0 ("a", False, 'x', (), LT, 6) == 6)
                        (and
                            (foldl (+) 0 ("a", False, 'x', (), LT, Just 1, 7) == 7)
                            (foldl (+) 0 ("a", False, 'x', (), LT, Just 1, [2], 8) == 8))))))

traversableTupleLaws : Bool
traversableTupleLaws =
    and
        (traverse Just ("a", 2) == Just ("a", 2))
        (and
            (traverse Just ("a", False, 3) == Just ("a", False, 3))
            (and
                (traverse Just ("a", False, 'x', 4) == Just ("a", False, 'x', 4))
                (and
                    (traverse Just ("a", False, 'x', (), 5) == Just ("a", False, 'x', (), 5))
                    (and
                        (traverse Just ("a", False, 'x', (), LT, 6) ==
                            Just ("a", False, 'x', (), LT, 6))
                        (and
                            (traverse Just ("a", False, 'x', (), LT, Just 1, 7) ==
                            Just ("a", False, 'x', (), LT, Just 1, 7))
                            (traverse Just ("a", False, 'x', (), LT, Just 1, [2], 8) ==
                            Just ("a", False, 'x', (), LT, Just 1, [2], 8)))))))

hashTupleLaws : Bool
hashTupleLaws =
    and
        (hashCoherent (1, 2) (1, 2))
        (and
            (hashCoherent (1, 2, 3) (1, 2, 3))
            (and
                (hashCoherent (1, 2, 3, 4) (1, 2, 3, 4))
                (and
                    (hashCoherent (1, 2, 3, 4, 5) (1, 2, 3, 4, 5))
                    (and
                        (hashCoherent (1, 2, 3, 4, 5, 6) (1, 2, 3, 4, 5, 6))
                        (and
                            (hashCoherent (1, 2, 3, 4, 5, 6, 7) (1, 2, 3, 4, 5, 6, 7))
                            (hashCoherent (1, 2, 3, 4, 5, 6, 7, 8) (1, 2, 3, 4, 5, 6, 7, 8)))))))

traversedLeft : Either String (Either String Int)
traversedLeft = traverse (fun value => Right (value + 1)) (Left "failure")

traversedRight : Either String (Either String Int)
traversedRight = traverse (fun value => Right (value + 1)) (Right 4)

expectedTraversedLeft : Either String (Either String Int)
expectedTraversedLeft = Right (Left "failure")

expectedTraversedRight : Either String (Either String Int)
expectedTraversedRight = Right (Right 5)

hashedLeft : Either String Int
hashedLeft = Left "failure"

rightValue : Either String Int
rightValue = Right 1

emptyMaybeInt : Maybe Int
emptyMaybeInt = Nothing

main : Task ()
main = do
    let values = [1, 2, 3]
    check
        "Foldable order"
        (and
            (foldr (fun value suffix => show value <> suffix) "" values == "123")
            (and
                (foldl (fun prefix value => prefix <> show value) "" values == "123")
                (foldMap show values == "123")))
    check
        "Foldable containers"
        (and
            (count (Just 1) == 1)
            (and
                (count emptyMaybeInt == 0)
                (and (count rightValue == 1) (and (count hashedLeft == 0) foldableTupleLaws))))
    check
        "Traversable identity and failure"
        (and
            (traverse Just values == Just values)
            (and
                (traverse maybeStep values == Just [11, 12, 13])
                (and
                    (traverse maybeStep [1, 4, 2] == Nothing)
                    (and (sequenceA [Just 1, Just 2] == Just [1, 2]) traversableTupleLaws))))
    check
        "Traversable naturality"
        (maybeToEither (traverse maybeStep values) == traverse (elementToEither . maybeStep) values)
    check "Traversable composition" (traverse compositionStep values == compositionRight values)
    check
        "Traversable Either"
        (and (traversedLeft == expectedTraversedLeft) (traversedRight == expectedTraversedRight))
    check
        "Hashable primitives"
        (and
            (hashCoherent 42 42)
            (and
                (hashCoherent 'x' 'x')
                (and
                    (hashCoherent "text" "text")
                    (and
                        (hashCoherent () ())
                        (and (hashCoherent True True) (hashCoherent LT LT))))))
    check
        "Hashable containers"
        (and
            (hashCoherent (Just [1, 2]) (Just [1, 2]))
            (and
                (hashCoherent hashedLeft hashedLeft)
                (and (hashWithSalt 99 values == hashWithSalt 99 values) hashTupleLaws)))
    let large = buildRange 20000 []
    check
        "stack-safe folds"
        (and (count large == 20000) (foldr (fun _ total => total + 1) 0 large == 20000))
    case traverse Just large of
        Nothing => error "law failed: stack-safe traversal"
        | Just traversed => check "stack-safe traversal" (count traversed == 20000)
    traversedIO <- traverse visit values
    check "IO traversal order" (traversedIO == [2, 4, 6])
