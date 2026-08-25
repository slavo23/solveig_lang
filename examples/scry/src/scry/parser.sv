import std.list(*)
import std.text(stringToChars, charsToString)
import scry.ast(*)

isWhitespace : Char -> Bool
isWhitespace c =
    if c == ' ' then True
    else if c == '\n' then True
    else if c == '\t' then True
    else False

flushAtom : [Char] -> [Tok] -> [Tok]
flushAtom current tokens =
    case current of
        [] => tokens
        | _ => TAtom (charsToString (reverse current)) :: tokens

tokenizeChars : [Char] -> [Char] -> [Tok] -> [Tok]
tokenizeChars chars current tokens =
    case chars of
        [] => reverse (flushAtom current tokens)
        | c :: rest =>
            if isWhitespace c then
                tokenizeChars rest [] (flushAtom current tokens)
            else if c == '(' then
                tokenizeChars rest [] (TLParen :: flushAtom current tokens)
            else if c == ')' then
                tokenizeChars rest [] (TRParen :: flushAtom current tokens)
            else
                tokenizeChars rest (c :: current) tokens

tokenize : String -> [Tok]
tokenize text = tokenizeChars (stringToChars text) [] []

parseList : [Tok] -> [SExp] -> ParseStep
parseList tokens items =
    case tokens of
        [] => ParseError
        | TRParen :: rest => Parsed (SList (reverse items)) rest
        | _ =>
            case parseSExp tokens of
                ParseError => ParseError
                | Parsed item rest => parseList rest (item :: items)

parseSExp : [Tok] -> ParseStep
parseSExp tokens =
    case tokens of
        [] => ParseError
        | TAtom value :: rest => Parsed (SAtom value) rest
        | TLParen :: rest => parseList rest []
        | TRParen :: rest => ParseError

parseAllTokens : [Tok] -> Maybe [SExp]
parseAllTokens tokens =
    case tokens of
        [] => Just []
        | _ =>
            case parseSExp tokens of
                ParseError => Nothing
                | Parsed item rest =>
                    case parseAllTokens rest of
                        Nothing => Nothing
                        | Just items => Just (item :: items)

isVariableName : String -> Bool
isVariableName name =
    case stringToChars name of
        '?' :: _ => True
        | _ => False

termFromSExp : SExp -> Maybe Term
termFromSExp sexp =
    case sexp of
        SAtom name =>
            if isVariableName name then
                Just (Var name)
            else
                Just (Atom name)
        | SList _ => Nothing

termsFromSExps : [SExp] -> Maybe [Term]
termsFromSExps sexps =
    case sexps of
        [] => Just []
        | item :: rest =>
            case termFromSExp item of
                Nothing => Nothing
                | Just term =>
                    case termsFromSExps rest of
                        Nothing => Nothing
                        | Just terms => Just (term :: terms)

predFromSExp : SExp -> Maybe Pred
predFromSExp sexp =
    case sexp of
        SList parts =>
            case parts of
                SAtom name :: args =>
                    case termsFromSExps args of
                        Nothing => Nothing
                        | Just terms => Just (Pred name terms)
                | _ => Nothing
        | SAtom _ => Nothing

goalsFromSExps : [SExp] -> Maybe [Goal]
goalsFromSExps sexps =
    case sexps of
        [] => Just []
        | item :: rest =>
            case goalFromSExp item of
                Nothing => Nothing
                | Just goal =>
                    case goalsFromSExps rest of
                        Nothing => Nothing
                        | Just goals => Just (goal :: goals)

goalFromSExp : SExp -> Maybe Goal
goalFromSExp sexp =
    case sexp of
        SList parts =>
            case parts of
                SAtom "and" :: goals =>
                    case goalsFromSExps goals of
                        Nothing => Nothing
                        | Just parsed => Just (GoalAnd parsed)
                | SAtom "or" :: goals =>
                    case goalsFromSExps goals of
                        Nothing => Nothing
                        | Just parsed => Just (GoalOr parsed)
                | _ =>
                    case predFromSExp sexp of
                        Nothing => Nothing
                        | Just pred => Just (GoalPred pred)
        | SAtom _ => Nothing

clauseFromSExp : SExp -> Maybe Clause
clauseFromSExp sexp =
    case sexp of
        SList parts =>
            case parts of
                SAtom "fact" :: predForm :: [] =>
                    case predFromSExp predForm of
                        Nothing => Nothing
                        | Just pred => Just (Clause pred GoalTrue)
                | SAtom "rule" :: headForm :: bodyForm :: [] =>
                    case predFromSExp headForm of
                        Nothing => Nothing
                        | Just head =>
                            case goalFromSExp bodyForm of
                                Nothing => Nothing
                                | Just body => Just (Clause head body)
                | _ => Nothing
        | SAtom _ => Nothing

clausesFromSExps : [SExp] -> Maybe [Clause]
clausesFromSExps sexps =
    case sexps of
        [] => Just []
        | item :: rest =>
            case clauseFromSExp item of
                Nothing => Nothing
                | Just clause =>
                    case clausesFromSExps rest of
                        Nothing => Nothing
                        | Just clauses => Just (clause :: clauses)

parseProgram : String -> Maybe [Clause]
parseProgram text =
    case parseAllTokens (tokenize text) of
        Nothing => Nothing
        | Just sexps => clausesFromSExps sexps

parseQuery : String -> Maybe Goal
parseQuery text =
    case parseAllTokens (tokenize text) of
        Just (sexp :: []) => goalFromSExp sexp
        | _ => Nothing
