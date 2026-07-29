import hoverLib(documentedIncrement, Documented, MkDocumented, Describe)

localValue : Int
localValue =
    let inner = 20 in
    inner + 1

topLevel : Int -> Int
topLevel n = documentedIncrement n

data LocalBox = MkLocalBox Int

useImported : Documented -> Documented
useImported value =
    case value of
        MkDocumented n => MkDocumented (documentedIncrement n)

useDescriber : forall a. Describe a => a -> String
useDescriber value = describe value

maybeResult : Maybe Int
maybeResult = do
    x <- Just 2
    y <- Just 40
    pure (x + y)

main : Int
main =
    case MkLocalBox 41 of
        MkLocalBox value => topLevel value
