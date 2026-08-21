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

-- | Reports whether text begins with an exact prefix.
startsWith : String -> String -> Bool
startsWith prefix text =
    let prefixLength = length prefix in
    and
        (length text >= prefixLength)
        (prelude.prim.stringSlice text 0 prefixLength == prefix)

-- | Removes an exact prefix when it is present.
stripPrefix : String -> String -> Maybe String
stripPrefix prefix text =
    if startsWith prefix text then
        let prefixLength = length prefix in
        Just (prelude.prim.stringSlice text prefixLength (length text - prefixLength))
    else Nothing

-- | Removes ASCII spaces, tabs, carriage returns, and newlines at both ends.
trimAscii : String -> String
trimAscii text =
    charsToString
        (reverse
            (trimAsciiStart
                (reverse (trimAsciiStart (stringToChars text)))))

trimAsciiStart : [Char] -> [Char]
trimAsciiStart [] = []
trimAsciiStart (character :: rest) =
    if isAsciiWhitespace character then trimAsciiStart rest
    else character :: rest

isAsciiWhitespace : Char -> Bool
isAsciiWhitespace character =
    let point = charToCodePoint character in
    or (point == 32) (or (point == 9) (or (point == 10) (point == 13)))

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
splitOnce : Char -> String -> Maybe (String, String)
splitOnce delimiter text = splitOnceFrom delimiter (stringToChars text) []

splitOnceFrom : Char -> [Char] -> [Char] -> Maybe (String, String)
splitOnceFrom _ [] _ = Nothing
splitOnceFrom delimiter (character :: rest) reversedLeft =
    if character == delimiter then
        Just (charsToString (reverse reversedLeft), charsToString rest)
    else
        splitOnceFrom delimiter rest (character :: reversedLeft)
