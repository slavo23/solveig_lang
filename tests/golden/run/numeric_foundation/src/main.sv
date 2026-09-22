import std.math(approximatelyEqual, isFinite, isInfinite, isNaN, pi, sqrt, toDegrees, toRadians)
import std.bigint(parse, parseBase, toFloat, toInt, toStringBase)

smallBig : BigInt
smallBig = 42

huge : BigInt
huge = 9223372036854775808

intDivision = (div (-7) 3, mod (-7) 3, quot (-7) 3, rem (-7) 3)

bigDivision =
    (
        div (negate huge) (toBigInt 3),
        mod (negate huge) (toBigInt 3),
        quot (negate huge) (toBigInt 3),
        rem (negate huge) (toBigInt 3)
    )

isZeroBig : BigInt -> Bool
isZeroBig 0 = True
isZeroBig _ = False

mathChecks =
    [
        sqrt 81.0 == 9.0,
        approximatelyEqual 0.000000000001 0.000000000001 (toDegrees pi) 180.0,
        approximatelyEqual 0.000000000001 0.000000000001 (toRadians 180.0) pi,
        isNaN (sqrt (-1.0)),
        isInfinite (toFloat (huge ^ 100)),
        isFinite pi,
        isZeroBig (toBigInt 0),
        not (isZeroBig huge)
    ]

contextualChecks =
    (1 + huge, approximatelyEqual 0.000000000001 0.000000000001 (2 / 3) 0.6666666666666666)

main =
    (
        (smallBig, huge + smallBig, huge ^ 2, contextualChecks),
        (
            intDivision,
            (
                bigDivision,
                (
                    toInt smallBig,
                    (
                        toInt (huge ^ 2),
                        (
                            parse "-12345678901234567890",
                            (parseBase 16 "ff", (toStringBase 2 smallBig, mathChecks))
                        )
                    )
                )
            )
        )
    )
