data Flag = Off | On

class Value a where
    value : a -> Int

instance Value Flag where
    value _ = 1
    value On = 2

main = value On
