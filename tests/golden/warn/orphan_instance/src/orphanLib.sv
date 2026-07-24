data ForeignBox = ForeignBox Int

class ForeignShow a where
    rank : a -> Int
