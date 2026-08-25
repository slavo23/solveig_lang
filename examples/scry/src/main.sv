import std.io(*)
import scry.ast(*)
import scry.parser(*)
import scry.pretty(*)
import scry.solver(*)
import scry.util(*)

helpText : String
helpText =
    appendString
        "Scry commands:\n"
        (appendString
            "  :help          show this help\n"
            (appendString
                "  :rules         print loaded facts and rules\n"
                (appendString
                    "  :quit          exit\n"
                    "  (goal args)    ask a query, using ?name for variables\n")))

welcome : [Clause] -> String
welcome clauses =
    appendString
        "Scry loaded "
        (appendString
            (show (lengthList clauses))
            " facts/rules. Type :help for commands.")

answerQuery : [Clause] -> Goal -> String
answerQuery clauses goal =
    renderResults (varsInGoal goal) (solveQuery clauses goal)

handleQuery : [Clause] -> String -> Task ()
handleQuery clauses line =
    case parseQuery line of
        Nothing => printLine "Could not parse query. Try :help."
        | Just goal => printStr (answerQuery clauses goal)

repl : [Clause] -> Task ()
repl clauses =
    do
        printStr "scry> "
        line <- readLine
        if line == "" then
            printLine "bye"
        else if line == ":quit" then
            printLine "bye"
        else if line == ":help" then
            do
                printStr helpText
                repl clauses
        else if line == ":rules" then
            do
                printStr (renderProgram clauses)
                repl clauses
        else
            do
                handleQuery clauses line
                repl clauses

main : Task ()
main =
    do
        contents <- readFile "knowledge.scry"
        case parseProgram contents of
            Nothing => printLine "Could not parse knowledge.scry"
            | Just clauses =>
                do
                    printLine (welcome clauses)
                    repl clauses
