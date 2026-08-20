import prelude.prim
import std.list(reverse)

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

-- | Returns the number of Unicode scalar values in a string.
length : String -> Int
length text = prelude.prim.stringLength text

-- | Reports whether a string contains no Unicode scalar values.
isEmpty : String -> Bool
isEmpty text = length text == 0

-- | Splits text at newline characters without retaining the separators.
-- |
-- | A trailing newline does not add a final empty line; empty lines elsewhere
-- | are preserved.
lines : String -> [String]
lines text = linesFrom (stringToChars text) []

linesFrom : [Char] -> [Char] -> [String]
linesFrom [] [] = []
linesFrom [] reversedLine = [charsToString (reverse reversedLine)]
linesFrom (character :: rest) reversedLine =
    if character == '\n' then
        charsToString (reverse reversedLine) :: linesFrom rest []
    else
        linesFrom rest (character :: reversedLine)

-- | Splits text around the first occurrence of a delimiter.
splitOnce : Char -> String -> Maybe (Pair String String)
splitOnce delimiter text = splitOnceFrom delimiter (stringToChars text) []

splitOnceFrom : Char -> [Char] -> [Char] -> Maybe (Pair String String)
splitOnceFrom _ [] _ = Nothing
splitOnceFrom delimiter (character :: rest) reversedLeft =
    if character == delimiter then
        Just (MkPair (charsToString (reverse reversedLeft)) (charsToString rest))
    else
        splitOnceFrom delimiter rest (character :: reversedLeft)
