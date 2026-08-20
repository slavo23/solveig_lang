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

-- | Returns the Unicode scalar value represented by a character.
charToCodePoint : Char -> Int
charToCodePoint character = prelude.prim.charToCodePoint character

-- | Builds a one-scalar string when an integer is a valid Unicode scalar value.
-- |
-- | Surrogate code points, negative integers, and values above U+10FFFF return
-- | Nothing.
codePointToString : Int -> Maybe String
codePointToString codePoint = prelude.prim.codePointToString codePoint
