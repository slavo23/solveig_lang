infixl 5 `mix`

mix x y = x + y
left = (1 +)
right = (+ 1)
named = 1 `mix` 2
neg = (- 1)
pattern =
    case MkPair 1 2 of
        left `MkPair` right => left + right
