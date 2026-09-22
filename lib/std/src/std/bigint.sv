export (fromInt, toInt, toFloat, toString, parse, parseBase, toStringBase)

import prelude.prim(
    bigIntFromInt,
    bigIntParse,
    bigIntParseBase,
    bigIntToFloat,
    bigIntToInt,
    bigIntToString,
    bigIntToStringBase
)

-- | Converts a fixed-width Int to an exact arbitrary-precision integer.
fromInt : Int -> BigInt
fromInt = bigIntFromInt

-- | Converts a BigInt when its exact value fits in the signed 64-bit Int range.
toInt : BigInt -> Maybe Int
toInt = bigIntToInt

-- | Converts a BigInt to the nearest representable Float.
-- | Values beyond Float's finite range become signed infinity.
toFloat : BigInt -> Float
toFloat = bigIntToFloat

-- | Renders a BigInt as ordinary base-10 digits.
toString : BigInt -> String
toString = bigIntToString

-- | Parses an optional sign followed by base-10 digits.
-- | Leading/trailing whitespace and separators are rejected.
parse : String -> Maybe BigInt
parse = bigIntParse

-- | Parses an optional sign followed by digits in a base from 2 through 36.
-- | Invalid bases and invalid digits return Nothing.
parseBase : Int -> String -> Maybe BigInt
parseBase = bigIntParseBase

-- | Renders in lowercase digits using a base from 2 through 36.
-- | Invalid bases return Nothing.
toStringBase : Int -> BigInt -> Maybe String
toStringBase = bigIntToStringBase
