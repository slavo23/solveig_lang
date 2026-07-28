class Shadow a where
    shadow : forall a. a -> a

instance Shadow Int where
    shadow x = x

main : Int
main = 42
