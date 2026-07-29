import semanticLib(Documented, MkDocumented, Describe, describe, documentedIncrement)

-- semantic coverage comment
data LocalBox a = MkLocalBox a

type LocalAlias a = LocalBox a

class LocalShow a where
    localShow : a -> String

identity : forall a. a -> a
identity value = value

useImported : Documented -> Int
useImported wrapped =
    case wrapped of
        MkDocumented n => documentedIncrement n

useDo : Maybe Int
useDo = do
    x <- Just 1
    y <- Just 2
    pure (x + y)

literalString : String
literalString = "solveig"

main : Int
main = 42
