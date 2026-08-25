import scry.ast(*)
import scry.solver(*)
import scry.util(*)

renderTerm : Term -> String
renderTerm term =
    case term of
        Atom value => value
        | Var name => name

renderTerms : [Term] -> [String]
renderTerms terms =
    case terms of
        [] => []
        | term :: rest => renderTerm term :: renderTerms rest

renderPred : Pred -> String
renderPred pred =
    case pred of
        Pred name terms =>
            appendString
                "("
                (appendString
                    (joinWith " " (name :: renderTerms terms))
                    ")")

renderGoal : Goal -> String
renderGoal goal =
    case goal of
        GoalTrue => "true"
        | GoalPred pred => renderPred pred
        | GoalAnd goals =>
            appendString "(and " (appendString (joinWith " " (renderGoals goals)) ")")
        | GoalOr goals =>
            appendString "(or " (appendString (joinWith " " (renderGoals goals)) ")")

renderGoals : [Goal] -> [String]
renderGoals goals =
    case goals of
        [] => []
        | goal :: rest => renderGoal goal :: renderGoals rest

renderClause : Clause -> String
renderClause clause =
    case clause of
        Clause head GoalTrue =>
            appendString "(fact " (appendString (renderPred head) ")")
        | Clause head body =>
            appendString
                "(rule "
                (appendString
                    (renderPred head)
                    (appendString " " (appendString (renderGoal body) ")")))

renderClauses : [Clause] -> [String]
renderClauses clauses =
    case clauses of
        [] => []
        | clause :: rest => renderClause clause :: renderClauses rest

renderProgram : [Clause] -> String
renderProgram clauses = linesOf (renderClauses clauses)

renderBinding : [Binding] -> String -> String
renderBinding bindings name =
    appendString
        name
        (appendString " = " (renderTerm (walk bindings (Var name))))

renderBindings : [Binding] -> [String] -> [String]
renderBindings bindings names =
    case names of
        [] => []
        | name :: rest => renderBinding bindings name :: renderBindings bindings rest

renderSolution : [String] -> [Binding] -> String
renderSolution names bindings =
    case names of
        [] => "true"
        | _ => joinWith ", " (renderBindings bindings names)

renderSolutionLines : [String] -> [[Binding]] -> String
renderSolutionLines names solutions =
    case solutions of
        [] => ""
        | bindings :: rest =>
            appendString
                (renderSolution names bindings)
                (appendString "\n" (renderSolutionLines names rest))

renderResults : [String] -> [[Binding]] -> String
renderResults names solutions =
    case solutions of
        [] => "false\n"
        | _ =>
            case names of
                [] => "true\n"
                | _ => renderSolutionLines names solutions
