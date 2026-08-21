infixr 8 `raise`
infixl 6 `mix`

data Joined a b = Joined a b

raise : Int -> Int -> Int
raise x y = x * 100 + y

mix : Int -> Int -> Int
mix x y = x * 10 + y

leftSection : Int
leftSection = (10 +) 7

rightSection : Int
rightSection = (+ 5) 7

minusNegation : Int
minusNegation = (- 5)

minusLeftSection : Int
minusLeftSection = (20 -) 3

subtractSection : Int
subtractSection = (subtract 4) 10

rightNamedFixity : Int
rightNamedFixity = 1 `raise` 2 `raise` 3

leftNamedFixity : Int
leftNamedFixity = 1 `mix` 2 `mix` 3

mixedFixity : Int
mixedFixity = 2 + 3 `raise` 4

patternBacktick : Int
patternBacktick =
    case Joined 7 8 of
        left `Joined` right => left + right

main : Int
main =
    leftSection
    + rightSection
    + minusNegation
    + minusLeftSection
    + subtractSection
    + rightNamedFixity
    + leftNamedFixity
    + mixedFixity
    + patternBacktick
