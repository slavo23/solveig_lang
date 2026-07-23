class Local a where
    local : a -> Int

instance Local Int where
    local x = 1

instance Local Int where
    local x = 2

main = local 1
