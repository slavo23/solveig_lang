main : IO ()
main =
    seqIO
        (printStr "alpha")
        (seqIO (printStr "-") (printLine "omega"))
