class Local a where
    local : a -> Int

instance Local Int where
    local x = True

main = local 1
