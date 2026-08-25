export (
    stringToChars, charsToString, intToString, charToCodePoint, codePointToString,
    length, byteLength, isEmpty, charAt, slice, byteSlice, startsWith, stripPrefix,
    trimAscii, lines, splitOnce, TextBuilder, emptyTextBuilder, textBuilder,
    charBuilder, appendTextBuilder, buildText
)

import prelude.prim(*)
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
-- |
-- | Combining sequences count once per scalar value, not once per displayed
-- | grapheme cluster. The packed UTF-8 text is scanned without constructing a
-- | list of characters.
length : String -> Int
length text = prelude.prim.stringLength text

-- | Returns the number of bytes in a string's UTF-8 encoding in constant time.
byteLength : String -> Int
byteLength text = prelude.prim.stringByteLength text

-- | Reports whether a string contains no UTF-8 bytes.
isEmpty : String -> Bool
isEmpty text = byteLength text == 0

-- | Returns the Unicode scalar value at a zero-based scalar index.
-- |
-- | Negative and out-of-range indices return Nothing. Indexing scans from the
-- | start of the packed text and does not allocate.
charAt : Int -> String -> Maybe Char
charAt index text = prelude.prim.stringCharAt text index

-- | Takes a scalar-indexed substring, clamping it to the available text.
-- |
-- | Negative starts behave as zero and non-positive lengths return empty text.
-- | The operation scans packed UTF-8 and allocates only the returned String.
slice : Int -> Int -> String -> String
slice start count text = prelude.prim.stringSlice text start count

-- | Takes an exact byte-indexed substring when both endpoints are UTF-8 boundaries.
-- |
-- | Negative, overflowing, out-of-range, or split-scalar ranges return Nothing.
-- | A successful result allocates only the returned String.
byteSlice : Int -> Int -> String -> Maybe String
byteSlice start count text = prelude.prim.stringByteSlice text start count

-- | Reports whether text begins with an exact prefix.
startsWith : String -> String -> Bool
startsWith prefix text = prelude.prim.stringStartsWith text prefix

-- | Removes an exact prefix when it is present.
stripPrefix : String -> String -> Maybe String
stripPrefix prefix text =
    if startsWith prefix text then
        let prefixBytes = byteLength prefix in
        byteSlice prefixBytes (byteLength text - prefixBytes) text
    else Nothing

-- | Removes ASCII spaces, tabs, carriage returns, and newlines at both ends.
trimAscii : String -> String
trimAscii text = prelude.prim.stringTrimAscii text

-- | Splits text at newline characters without retaining the separators.
-- |
-- | A trailing newline does not add a final empty line; empty lines elsewhere
-- | are preserved.
lines : String -> [String]
lines text = reverse (linesFrom text 0 [])

linesFrom : String -> Int -> [String] -> [String]
linesFrom text start reversed =
    let total = byteLength text in
    if start >= total then reversed
    else
        let end = prelude.prim.stringFindCharFrom text start '\n' in
        case byteSlice start (end - start) text of
            Nothing => prelude.prim.error "std.text.lines produced an invalid UTF-8 boundary"
            | Just line =>
                if end == total then line :: reversed
                else linesFrom text (end + 1) (line :: reversed)

-- | Splits text around the first occurrence of a delimiter.
splitOnce : Char -> String -> Maybe (String, String)
splitOnce delimiter text =
    let position = prelude.prim.stringFindCharFrom text 0 delimiter in
    let total = byteLength text in
    if position == total then Nothing
    else
        let next = position + charByteLength delimiter in
        case (byteSlice 0 position text, byteSlice next (total - next) text) of
            (Just left, Just right) => Just (left, right)
            | _ => prelude.prim.error "std.text.splitOnce produced an invalid UTF-8 boundary"

charByteLength : Char -> Int
charByteLength character =
    let point = charToCodePoint character in
    if point <= 127 then 1
    else if point <= 2047 then 2
    else if point <= 65535 then 3
    else 4

-- | A compositional plan for constructing one String without repeated copying.
-- |
-- | Builder concatenation takes constant time. buildText traverses all chunks
-- | without consuming the call stack and allocates one packed result.
data TextBuilder =
    EmptyTextBuilder
  | TextChunk String
  | AppendTextBuilder TextBuilder TextBuilder

instance Semigroup TextBuilder where
    combine = appendTextBuilder

instance Monoid TextBuilder where
    empty = EmptyTextBuilder

-- | Returns an empty text builder.
emptyTextBuilder : TextBuilder
emptyTextBuilder = EmptyTextBuilder

-- | Builds a text builder containing one existing String without copying it.
textBuilder : String -> TextBuilder
textBuilder text =
    if isEmpty text then EmptyTextBuilder else TextChunk text

-- | Builds a text builder containing one Unicode scalar value.
charBuilder : Char -> TextBuilder
charBuilder character = TextChunk (prelude.prim.charToString character)

-- | Concatenates two text builders in constant time while preserving order.
appendTextBuilder : TextBuilder -> TextBuilder -> TextBuilder
appendTextBuilder EmptyTextBuilder right = right
appendTextBuilder left EmptyTextBuilder = left
appendTextBuilder left right = AppendTextBuilder left right

-- | Constructs the builder's packed String in time linear in its UTF-8 bytes.
-- |
-- | The traversal is stack-safe and allocates a temporary list of chunk
-- | references plus one result String.
buildText : TextBuilder -> String
buildText builder = prelude.prim.stringsConcat (builderChunks builder)

builderChunks : TextBuilder -> [String]
builderChunks builder = reverse (builderChunksInto [builder] [])

builderChunksInto : [TextBuilder] -> [String] -> [String]
builderChunksInto [] reversed = reversed
builderChunksInto (EmptyTextBuilder :: rest) reversed =
    builderChunksInto rest reversed
builderChunksInto (TextChunk text :: rest) reversed =
    builderChunksInto rest (text :: reversed)
builderChunksInto (AppendTextBuilder left right :: rest) reversed =
    builderChunksInto (left :: right :: rest) reversed
