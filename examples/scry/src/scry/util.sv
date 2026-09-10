containsString : String -> [String] -> Bool
containsString needle haystack =
    case haystack of
        [] => False
        | x :: rest => if x == needle then True else containsString needle rest

uniqueCons : String -> [String] -> [String]
uniqueCons value values = if containsString value values then values else value :: values

uniqueAppend : [String] -> [String] -> [String]
uniqueAppend xs ys =
    case xs of
        [] => ys
        | x :: rest => uniqueCons x (uniqueAppend rest ys)

concatMap : forall a b. (a -> [b]) -> [a] -> [b]
concatMap f xs = concat (map f xs)

joinWith : String -> [String] -> String
joinWith separator pieces =
    case pieces of
        [] => ""
        | piece :: rest =>
            case rest of
                [] => piece
                | _ => appendString piece (appendString separator (joinWith separator rest))

linesOf : [String] -> String
linesOf lines =
    case lines of
        [] => ""
        | line :: rest => appendString line (appendString "\n" (linesOf rest))

lengthList : forall a. [a] -> Int
lengthList xs =
    case xs of
        [] => 0
        | _ :: rest => 1 + lengthList rest
