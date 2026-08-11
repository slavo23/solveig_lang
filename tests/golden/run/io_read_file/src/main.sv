main : Task ()
main =
    do
        contents <- readFile "questions.txt"
        printStr contents
