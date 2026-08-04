data Box = MkBox Int

class Need a where
    need : a -> Int

value = 2
