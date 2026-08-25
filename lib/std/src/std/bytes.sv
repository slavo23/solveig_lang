export (
    stringToBytes, bytesToString, bytesLength, bytesIndex, bytesSlice, bytesAppend,
    bytesSpanAny, bytesFindAny, bytesFindBelow, bytesSpanRange, bytesSliceToString
)

import prelude.prim(*)

-- | Encodes text as UTF-8 bytes.
stringToBytes : String -> Bytes
stringToBytes text = prelude.prim.stringToBytes text

-- | Decodes UTF-8 bytes without replacing invalid input.
bytesToString : Bytes -> Result IOError String
bytesToString bytes = prelude.prim.bytesToString bytes

-- | Returns the number of bytes.
bytesLength : Bytes -> Int
bytesLength bytes = prelude.prim.bytesLength bytes

-- | Returns the byte at an index when it exists.
bytesIndex : Bytes -> Int -> Maybe Int
bytesIndex bytes index = prelude.prim.bytesIndex bytes index

-- | Takes bytes from a starting index for a requested length.
bytesSlice : Bytes -> Int -> Int -> Bytes
bytesSlice bytes start length = prelude.prim.bytesSlice bytes start length

-- | Concatenates two byte strings.
bytesAppend : Bytes -> Bytes -> Bytes
bytesAppend left right = prelude.prim.bytesAppend left right

-- | Advances from an index while each byte occurs in an accepted byte set.
-- |
-- | The returned index is clamped to the source length. A negative start is
-- | treated as zero. The scan allocates no per-byte language values.
bytesSpanAny : Bytes -> Int -> Bytes -> Int
bytesSpanAny bytes start accepted = prelude.prim.bytesSpanAny bytes start accepted

-- | Finds the first byte at or after an index that occurs in a needle set.
-- |
-- | Returns the source length when no needle occurs. A negative start is
-- | treated as zero. The scan allocates no per-byte language values.
bytesFindAny : Bytes -> Int -> Bytes -> Int
bytesFindAny bytes start needles = prelude.prim.bytesFindAny bytes start needles

-- | Finds the first byte at or after an index below an exclusive limit.
-- |
-- | Returns the source length when no byte is below the limit. This is useful
-- | for bulk detection of ASCII control bytes.
bytesFindBelow : Bytes -> Int -> Int -> Int
bytesFindBelow bytes start exclusiveLimit =
    prelude.prim.bytesFindBelow bytes start exclusiveLimit

-- | Advances while bytes remain inside an inclusive integer range.
-- |
-- | The returned index is the first byte outside the range or the source
-- | length. Bounds outside 0 through 255 simply match no corresponding byte.
bytesSpanRange : Bytes -> Int -> Int -> Int -> Int
bytesSpanRange bytes start lower upper =
    prelude.prim.bytesSpanRange bytes start lower upper

-- | Strictly decodes one clamped byte slice as UTF-8 without allocating Bytes.
-- |
-- | Invalid UTF-8 returns InvalidUtf8. Negative starts and lengths behave like
-- | bytesSlice: the start is zero and the requested length is empty.
bytesSliceToString : Bytes -> Int -> Int -> Result IOError String
bytesSliceToString bytes start length =
    prelude.prim.bytesSliceToString bytes start length
