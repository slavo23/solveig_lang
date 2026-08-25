export (
    stringToBytes, bytesToString, bytesLength, bytesIndex, bytesSlice, bytesAppend,
    bytesToStringReplacing, bytesSpanAny, bytesFindAny, bytesFindBelow, bytesSpanRange,
    bytesSliceToString, BytesBuilder, emptyBytesBuilder, bytesBuilder,
    appendBytesBuilder, buildBytes
)

import prelude.prim(*)
import std.list(reverse)

-- | Encodes text as UTF-8 bytes.
stringToBytes : String -> Bytes
stringToBytes text = prelude.prim.stringToBytes text

-- | Decodes UTF-8 bytes without replacing invalid input.
bytesToString : Bytes -> Either Utf8Error String
bytesToString bytes = prelude.prim.bytesToString bytes

-- | Decodes UTF-8 bytes, replacing each malformed sequence with U+FFFD.
-- |
-- | Use bytesToString when invalid input must remain an explicit error.
bytesToStringReplacing : Bytes -> String
bytesToStringReplacing bytes = prelude.prim.bytesToStringReplacing bytes

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
bytesSliceToString : Bytes -> Int -> Int -> Either Utf8Error String
bytesSliceToString bytes start length =
    prelude.prim.bytesSliceToString bytes start length

-- | A compositional plan for constructing one Bytes value without repeated copying.
-- |
-- | Builder concatenation takes constant time. buildBytes traverses all chunks
-- | without consuming the call stack and allocates one packed result.
data BytesBuilder =
    EmptyBytesBuilder
  | BytesChunk Bytes
  | AppendBytesBuilder BytesBuilder BytesBuilder

instance Semigroup BytesBuilder where
    combine = appendBytesBuilder

instance Monoid BytesBuilder where
    empty = EmptyBytesBuilder

-- | Returns an empty byte builder.
emptyBytesBuilder : BytesBuilder
emptyBytesBuilder = EmptyBytesBuilder

-- | Builds a byte builder containing one existing Bytes value without copying it.
bytesBuilder : Bytes -> BytesBuilder
bytesBuilder bytes =
    if bytesLength bytes == 0 then EmptyBytesBuilder else BytesChunk bytes

-- | Concatenates two byte builders in constant time while preserving order.
appendBytesBuilder : BytesBuilder -> BytesBuilder -> BytesBuilder
appendBytesBuilder EmptyBytesBuilder right = right
appendBytesBuilder left EmptyBytesBuilder = left
appendBytesBuilder left right = AppendBytesBuilder left right

-- | Constructs the builder's packed Bytes value in time linear in its bytes.
-- |
-- | The traversal is stack-safe and allocates a temporary list of chunk
-- | references plus one result Bytes value.
buildBytes : BytesBuilder -> Bytes
buildBytes builder = prelude.prim.bytesConcat (builderChunks builder)

builderChunks : BytesBuilder -> [Bytes]
builderChunks builder = reverse (builderChunksInto [builder] [])

builderChunksInto : [BytesBuilder] -> [Bytes] -> [Bytes]
builderChunksInto [] reversed = reversed
builderChunksInto (EmptyBytesBuilder :: rest) reversed =
    builderChunksInto rest reversed
builderChunksInto (BytesChunk bytes :: rest) reversed =
    builderChunksInto rest (bytes :: reversed)
builderChunksInto (AppendBytesBuilder left right :: rest) reversed =
    builderChunksInto (left :: right :: rest) reversed
