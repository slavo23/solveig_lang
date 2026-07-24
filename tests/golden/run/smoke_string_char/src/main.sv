chooseText : String -> String
chooseText s =
    case s of
        "line\nquote\"" => s
      | _ => "miss"

chooseChar : Char -> Char
chooseChar c =
    case c of
        '\t' => c
      | _ => '?'

main : Pair String Char
main = MkPair (chooseText "line\nquote\"") (chooseChar '\t')
