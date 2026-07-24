data Box a = Box a

main : Box Int
main = do
    x <- Box 1
    Box x
