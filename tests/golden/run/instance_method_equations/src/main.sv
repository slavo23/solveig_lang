data Packet = Empty | One Int | Two Int Int

class Measure a where
    measure : a -> Int -> Int

instance Measure Packet where
    measure Empty bonus = bonus
    measure (One 0) bonus = 10 + bonus
    measure (One value) bonus = value + bonus
    measure (Two left right) bonus = left + right + bonus

main =
    measure Empty 1
        + measure (One 0) 2
        + measure (One 5) 3
        + measure (Two 4 6) 1
