data Term = Atom String | Var String

data Pred = Pred String [Term]

data Goal = GoalTrue | GoalPred Pred | GoalAnd [Goal] | GoalOr [Goal]

data Clause = Clause Pred Goal

data SExp = SAtom String | SList [SExp]

data Tok = TLParen | TRParen | TAtom String

data ParseStep = Parsed SExp [Tok] | ParseError
