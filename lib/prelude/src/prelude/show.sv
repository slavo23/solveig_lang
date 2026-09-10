export ()

import prelude.builtins(*)
import prelude.list(*)
import prelude.prim(*)
import prelude.typeclasses(*)

-- | Escapes a character for display inside character literals.
showCharPayload : Char -> String
showCharPayload c =
    if charEq c '\n'
        then "\\n"
        else if charEq c '\t'
                then "\\t"
                else if charEq c '\\'
                        then "\\\\"
                        else if charEq c '\''
                                then "\\'"
                                else if charEq c '"' then "\\\"" else charToString c

-- | Escapes a character for display inside string literals.
showStringPayload : Char -> String
showStringPayload c =
    if charEq c '\n'
        then "\\n"
        else if charEq c '\t'
                then "\\t"
                else if charEq c '\\'
                        then "\\\\"
                        else if charEq c '"' then "\\\"" else charToString c

-- | Renders the characters inside a string literal.
showStringChars : [Char] -> String
showStringChars [] = ""
showStringChars (c :: rest) = stringAppend (showStringPayload c) (showStringChars rest)

-- | Renders the comma-separated contents of a list.
showListItems : forall a. Show a => [a] -> String
showListItems [] = ""
showListItems (y :: []) = show y
showListItems (y :: ys) = stringAppend (show y) (stringAppend ", " (showListItems ys))
