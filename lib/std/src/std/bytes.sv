import prelude.prim

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
