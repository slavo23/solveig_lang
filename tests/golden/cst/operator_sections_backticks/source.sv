infixl 5 `mix`

data Joined a b = Joined a b

mix x y = x + y
left = (1 +)
right = (+ 1)
named = 1 `mix` 2
neg = (- 1)
pattern =
    case Joined 1 2 of
        left `Joined` right => left + right
