import orphanLib(ForeignBox, ForeignShow)

instance ForeignShow ForeignBox where
    rank box =
        case box of
            ForeignBox x => x

main : Int
main = rank (ForeignBox 5)
