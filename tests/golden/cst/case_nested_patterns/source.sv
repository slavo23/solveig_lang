data Tree = Leaf Int | Branch Tree Tree

main =
    case Branch (Leaf 1) (Leaf 2) of
        Leaf value => value
      | Branch (Leaf left) right =>
            case right of
                Leaf value => left + value
              | Branch _ _ => 0
