import a(Box, MkBox, Need, value)
import b(Box, MkBox, Need, value)

data Box = MkBox Int

class Need a where
    need : a -> Int

instance Need Box where
    need box =
        case box of
            MkBox n => n

value = 32

make : Box
make = MkBox 10

main : Int
main = need make + value
