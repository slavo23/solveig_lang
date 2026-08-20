data Bit = O | I

class Pick a where
    pick : a -> a -> Int

instance Pick Bit where
    pick O O = 0

main = pick O O
