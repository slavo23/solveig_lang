tick : Int -> Task ()
tick n =
    if n == 0
        then printLine "done"
        else do
                pureIO ()
                tick (n - 1)

main : Task ()
main = tick 1000000
