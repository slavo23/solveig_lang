main : IO ()
main =
    do
        contents <- readFile "questions.txt"
        printStr contents
