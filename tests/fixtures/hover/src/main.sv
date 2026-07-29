import hoverLib(documentedIncrement, Documented, MkDocumented, Describe)

localValue : Int
localValue =
    let inner = 20 in
    inner + 1

topLevel : Int -> Int
topLevel n = documentedIncrement n

data LocalBox = MkLocalBox Int

maybeResult : Maybe Int
maybeResult = do
    x <- Just 2
    y <- Just 40
    pure (x + y)

main : Int
main =
    case MkLocalBox 41 of
        MkLocalBox value => topLevel value
