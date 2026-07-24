import scry.ast
import scry.util

data Binding = Binding String Term

lookupBinding : String -> [Binding] -> Maybe Term
lookupBinding name bindings =
    case bindings of
        [] => Nothing
        | Binding key value :: rest =>
            if key == name then Just value else lookupBinding name rest

walk : [Binding] -> Term -> Term
walk bindings term =
    case term of
        Atom _ => term
        | Var name =>
            case lookupBinding name bindings of
                Nothing => term
                | Just next => walk bindings next

bindVar : String -> Term -> [Binding] -> Maybe [Binding]
bindVar name term bindings =
    case term of
        Var other =>
            if name == other then
                Just bindings
            else
                Just (Binding name term :: bindings)
        | Atom _ => Just (Binding name term :: bindings)

unifyTerm : Term -> Term -> [Binding] -> Maybe [Binding]
unifyTerm left right bindings =
    let walkedLeft = walk bindings left in
    let walkedRight = walk bindings right in
        case MkPair walkedLeft walkedRight of
            MkPair (Atom a) (Atom b) =>
                if a == b then Just bindings else Nothing
            | MkPair (Var name) term => bindVar name term bindings
            | MkPair term (Var name) => bindVar name term bindings

unifyArgs : [Term] -> [Term] -> [Binding] -> Maybe [Binding]
unifyArgs left right bindings =
    case left of
        [] =>
            case right of
                [] => Just bindings
                | _ :: _ => Nothing
        | l :: ls =>
            case right of
                [] => Nothing
                | r :: rs =>
                    case unifyTerm l r bindings of
                        Nothing => Nothing
                        | Just next => unifyArgs ls rs next

unifyPred : Pred -> Pred -> [Binding] -> Maybe [Binding]
unifyPred left right bindings =
    case left of
        Pred leftName leftArgs =>
            case right of
                Pred rightName rightArgs =>
                    if leftName == rightName then
                        unifyArgs leftArgs rightArgs bindings
                    else
                        Nothing

freshName : String -> Int -> String
freshName name depth =
    appendString name (appendString "#" (show depth))

freshTerm : Int -> Term -> Term
freshTerm depth term =
    case term of
        Atom _ => term
        | Var name => Var (freshName name depth)

freshTerms : Int -> [Term] -> [Term]
freshTerms depth terms =
    case terms of
        [] => []
        | term :: rest => freshTerm depth term :: freshTerms depth rest

freshPred : Int -> Pred -> Pred
freshPred depth pred =
    case pred of
        Pred name terms => Pred name (freshTerms depth terms)

freshGoal : Int -> Goal -> Goal
freshGoal depth goal =
    case goal of
        GoalTrue => GoalTrue
        | GoalPred pred => GoalPred (freshPred depth pred)
        | GoalAnd goals => GoalAnd (freshGoals depth goals)
        | GoalOr goals => GoalOr (freshGoals depth goals)

freshGoals : Int -> [Goal] -> [Goal]
freshGoals depth goals =
    case goals of
        [] => []
        | goal :: rest => freshGoal depth goal :: freshGoals depth rest

freshClause : Int -> Clause -> Clause
freshClause depth clause =
    case clause of
        Clause head body => Clause (freshPred depth head) (freshGoal depth body)

solveWithClause : [Clause] -> Int -> [Binding] -> Pred -> Clause -> [[Binding]]
solveWithClause rules depth bindings pred clause =
    case freshClause depth clause of
        Clause head body =>
            case unifyPred pred head bindings of
                Nothing => []
                | Just next => solveGoal rules (depth + 1) next body

solvePredLoop : [Clause] -> [Clause] -> Int -> [Binding] -> Pred -> [[Binding]]
solvePredLoop rules remaining depth bindings pred =
    case remaining of
        [] => []
        | clause :: rest =>
            append
                (solveWithClause rules depth bindings pred clause)
                (solvePredLoop rules rest (depth + 1) bindings pred)

solveAll : [Clause] -> Int -> [[Binding]] -> [Goal] -> [[Binding]]
solveAll rules depth bindings goals =
    case goals of
        [] => bindings
        | goal :: rest =>
            let next =
                concatMap
                    (fun current => solveGoal rules depth current goal)
                    bindings
            in
                solveAll rules (depth + 1) next rest

solveAny : [Clause] -> Int -> [Binding] -> [Goal] -> [[Binding]]
solveAny rules depth bindings goals =
    case goals of
        [] => []
        | goal :: rest =>
            append
                (solveGoal rules depth bindings goal)
                (solveAny rules (depth + 1) bindings rest)

solveGoal : [Clause] -> Int -> [Binding] -> Goal -> [[Binding]]
solveGoal rules depth bindings goal =
    case goal of
        GoalTrue => [bindings]
        | GoalPred pred => solvePredLoop rules rules depth bindings pred
        | GoalAnd goals => solveAll rules depth [bindings] goals
        | GoalOr goals => solveAny rules depth bindings goals

solveQuery : [Clause] -> Goal -> [[Binding]]
solveQuery rules goal = solveGoal rules 1 [] goal

varsInTerm : Term -> [String]
varsInTerm term =
    case term of
        Atom _ => []
        | Var name => [name]

varsInTerms : [Term] -> [String]
varsInTerms terms =
    case terms of
        [] => []
        | term :: rest => uniqueAppend (varsInTerm term) (varsInTerms rest)

varsInPred : Pred -> [String]
varsInPred pred =
    case pred of
        Pred _ terms => varsInTerms terms

varsInGoals : [Goal] -> [String]
varsInGoals goals =
    case goals of
        [] => []
        | goal :: rest => uniqueAppend (varsInGoal goal) (varsInGoals rest)

varsInGoal : Goal -> [String]
varsInGoal goal =
    case goal of
        GoalTrue => []
        | GoalPred pred => varsInPred pred
        | GoalAnd goals => varsInGoals goals
        | GoalOr goals => varsInGoals goals
