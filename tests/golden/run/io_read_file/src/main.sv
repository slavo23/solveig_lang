main : IO ()
main =
    bindIO (readFile "questions.txt") (fun contents => printStr contents)
