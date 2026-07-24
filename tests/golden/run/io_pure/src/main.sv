main : IO ()
main =
    bindIO (pureIO "pure-value") (fun value => printLine value)
