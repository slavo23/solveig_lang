export (
    pi,
    tau,
    e,
    square,
    cube,
    sqrt,
    cbrt,
    hypot,
    exp,
    exp2,
    ln,
    log2,
    log10,
    logBase,
    sin,
    cos,
    tan,
    asin,
    acos,
    atan,
    atan2,
    sinh,
    cosh,
    tanh,
    floor,
    ceiling,
    truncate,
    round,
    isNaN,
    isInfinite,
    isFinite,
    copySign,
    toDegrees,
    toRadians,
    approximatelyEqual,
    fromInt,
    truncateToInt,
    truncateToBigInt
)

private (maxFloat)

import prelude.prim(
    floatAbs,
    floatAcos,
    floatAsin,
    floatAtan,
    floatAtan2,
    floatCbrt,
    floatCeil,
    floatCopySign,
    floatCos,
    floatCosh,
    floatExp,
    floatExp2,
    floatFloor,
    floatHypot,
    floatIsFinite,
    floatIsInfinite,
    floatIsNaN,
    floatLog,
    floatLog10,
    floatLog2,
    floatPow,
    floatRound,
    floatSin,
    floatSinh,
    floatSqrt,
    floatTan,
    floatTanh,
    floatTruncate,
    floatToBigInt,
    floatToIntMaybe,
    intToFloat
)

-- | The ratio of a circle's circumference to its diameter.
pi : Float
pi = 3.141592653589793

-- | One full turn in radians (`2 * pi`).
tau : Float
tau = 6.283185307179586

-- | Euler's number, the base of the natural logarithm.
e : Float
e = 2.718281828459045

-- | Multiplies a number by itself.
square : forall a. Num a => a -> a
square x = x * x

-- | Raises a number to the third power.
cube : forall a. Num a => a -> a
cube x = x * x * x

-- | Returns the principal square root.
-- | Negative finite inputs produce NaN according to IEEE 754.
sqrt : Float -> Float
sqrt = floatSqrt

-- | Returns the real cube root, including for negative inputs.
cbrt : Float -> Float
cbrt = floatCbrt

-- | Computes `sqrt (x * x + y * y)` without avoidable intermediate overflow.
hypot : Float -> Float -> Float
hypot = floatHypot

-- | Computes `e` raised to a power.
exp : Float -> Float
exp = floatExp

-- | Computes 2 raised to a power.
exp2 : Float -> Float
exp2 = floatExp2

-- | Computes the natural logarithm.
ln : Float -> Float
ln = floatLog

-- | Computes the base-2 logarithm.
log2 : Float -> Float
log2 = floatLog2

-- | Computes the base-10 logarithm.
log10 : Float -> Float
log10 = floatLog10

-- | Computes a logarithm in the requested base.
-- | Invalid domains follow IEEE 754 and produce NaN or infinity.
logBase : Float -> Float -> Float
logBase base value = ln value / ln base

-- | Computes the sine of an angle in radians.
sin : Float -> Float
sin = floatSin

-- | Computes the cosine of an angle in radians.
cos : Float -> Float
cos = floatCos

-- | Computes the tangent of an angle in radians.
tan : Float -> Float
tan = floatTan

-- | Computes the inverse sine in radians.
asin : Float -> Float
asin = floatAsin

-- | Computes the inverse cosine in radians.
acos : Float -> Float
acos = floatAcos

-- | Computes the inverse tangent in radians.
atan : Float -> Float
atan = floatAtan

-- | Computes the quadrant-aware angle of `(y, x)` in radians.
atan2 : Float -> Float -> Float
atan2 y x = floatAtan2 y x

-- | Computes the hyperbolic sine.
sinh : Float -> Float
sinh = floatSinh

-- | Computes the hyperbolic cosine.
cosh : Float -> Float
cosh = floatCosh

-- | Computes the hyperbolic tangent.
tanh : Float -> Float
tanh = floatTanh

-- | Rounds downward and keeps the result as a Float.
floor : Float -> Float
floor = floatFloor

-- | Rounds upward and keeps the result as a Float.
ceiling : Float -> Float
ceiling = floatCeil

-- | Removes the fractional part by rounding toward zero.
truncate : Float -> Float
truncate = floatTruncate

-- | Rounds to the nearest integer-valued Float, with halfway cases away from zero.
round : Float -> Float
round = floatRound

-- | Reports whether a value is IEEE 754 not-a-number.
isNaN : Float -> Bool
isNaN = floatIsNaN

-- | Reports whether a value is positive or negative infinity.
isInfinite : Float -> Bool
isInfinite = floatIsInfinite

-- | Reports whether a value is neither infinity nor NaN.
isFinite : Float -> Bool
isFinite = floatIsFinite

-- | Returns the magnitude of the first value with the sign bit of the second.
copySign : Float -> Float -> Float
copySign = floatCopySign

-- | Converts degrees to radians.
toRadians : Float -> Float
toRadians value = value * pi / 180.0

-- | Converts radians to degrees.
toDegrees : Float -> Float
toDegrees value = value * 180.0 / pi

-- | Compares two finite values with absolute and relative tolerances.
-- |
-- | The absolute tolerance controls behavior near zero. The relative tolerance
-- | scales with the larger magnitude. NaN never compares approximately equal.
approximatelyEqual : Float -> Float -> Float -> Float -> Bool
approximatelyEqual absoluteTolerance relativeTolerance left right =
    floatAbs (left - right)
        <= maxFloat
            (floatAbs absoluteTolerance)
            (floatAbs relativeTolerance * maxFloat (floatAbs left) (floatAbs right))

-- | Converts Int to Float. Large magnitudes may round to the nearest Float.
fromInt : Int -> Float
fromInt = intToFloat

-- | Truncates toward zero when a finite Float fits in Int.
truncateToInt : Float -> Maybe Int
truncateToInt = floatToIntMaybe

-- | Truncates a finite Float toward zero as an arbitrary-precision integer.
truncateToBigInt : Float -> Maybe BigInt
truncateToBigInt = floatToBigInt

maxFloat : Float -> Float -> Float
maxFloat left right = if left >= right then left else right
