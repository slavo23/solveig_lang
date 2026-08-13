import prelude.prim

-- | Converts a string to its Unicode scalar values.
stringToChars : String -> [Char]
stringToChars text = prelude.prim.stringToChars text

-- | Builds a string from Unicode scalar values.
charsToString : [Char] -> String
charsToString characters = prelude.prim.charsToString characters

-- | Renders an integer as decimal text.
intToString : Int -> String
intToString value = prelude.prim.intToString value
