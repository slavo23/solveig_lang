tick : Int -> IO ()
tick n =
    if n == 0 then printLine "done"
    else do
        pureIO ()
        tick (n - 1)

main : IO ()
main = tick 1000000
