failAction : () -> Task ()
failAction _ =
    bindIO (pureIO ()) (fun _ =>
        bindIO (pureIO (div 1 0)) (fun _ =>
            pureIO ()))

wait : Int -> Task ()
wait n =
    if n == 0 then pureIO ()
    else do
        pureIO ()
        wait (n - 1)

main : Task ()
main = do
    _ <- spawn (failAction ())
    wait 10000
    printLine "main survived"
