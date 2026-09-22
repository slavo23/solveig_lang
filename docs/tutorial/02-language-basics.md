# 2. Language basics

Solveig programs are built from expressions. An `if`, a `case`, a function,
and a `let` all produce values; they are not statement-only forms.

## Primitive values

The commonly used primitive types are:

```solveig
anInteger : Int
anInteger = 42

anExactLargeInteger : BigInt
anExactLargeInteger = 1000000000000000000000000000000

aFloat : Float
aFloat = 2.5

aCharacter : Char
aCharacter = 'S'

someText : String
someText = "Solveig"

aFlag : Bool
aFlag = True

nothingUseful : ()
nothingUseful = ()
```

Strings support escapes such as `\n`, `\t`, `\"`, and Unicode scalar escapes
such as `\u{1f984}`. String concatenation is `<>`:

```solveig
message : String
message = "hello, " <> "world"
```

`Int` is a signed 64-bit integer. Its arithmetic is checked: a result outside
`-9223372036854775808` through `9223372036854775807` raises the
`IntegerOverflow` defect instead of silently wrapping. Use `BigInt` when the
mathematical result must grow without a fixed bound. `Float` is IEEE 754
binary64; it has finite precision and includes infinities and NaN. Solveig does
not currently provide an arbitrary-precision floating type.

## Choosing a numeric type

Whole-number literals use their context. A literal in a declared `BigInt` or
`Float` position becomes that type, while an unconstrained ordinary literal
defaults to `Int`. A whole literal outside the `Int` range starts as `BigInt`.
The checker waits for the surrounding numeric expression before applying that
default, so operand order does not change the result:

```solveig
requestCount : Int
requestCount = 42

worldPopulationEstimate : BigInt
worldPopulationEstimate = 8100000000

nextEstimate : BigInt
nextEstimate = 1 + worldPopulationEstimate

ratio : Float
ratio = 2 / 3
```

This is contextual literal typing, not implicit conversion of stored values.
Once a value is an `Int`, use `toBigInt`; once it is a `BigInt`, import
`std.bigint.toInt` and handle the possible overflow:

```solveig
import std.bigint(parse, toInt, toStringBase)

exact : BigInt
exact = toBigInt 42

narrowed : Maybe Int
narrowed = toInt exact

fromConfig : Maybe BigInt
fromConfig = parse "340282366920938463463374607431768211455"

asHex : Maybe String
asHex = toStringBase 16 exact
```

`+`, `-`, `*`, `negate`, `abs`, `signum`, and `^` use `Num`. The exponent of
`^` is a non-negative `Int`; a negative exponent is a defect, and an `Int`
power can still raise `IntegerOverflow`. `/` uses `Fractional`; `**` is
floating-point exponentiation.

Integral division has two deliberately named families. For negative inputs,
the difference matters:

```solveig
floorDivision = (div (-7) 3, mod (-7) 3) -- (-3, 2)
truncatingDivision = (quot (-7) 3, rem (-7) 3) -- (-2, -1)
```

`div` rounds toward negative infinity and `mod` has the divisor's sign.
`quot` rounds toward zero and `rem` has the dividend's sign. The `%` operator
is an alias for `mod`. All four raise `DivisionByZero` for a zero divisor.

For scientific and geometric work, import the named operations from
`std.math`:

```solveig
import std.math(approximatelyEqual, hypot, pi, sin, toDegrees)

diagonal : Float
diagonal = hypot 3.0 4.0

straightAngle : Float
straightAngle = toDegrees pi

closeEnough : Bool
closeEnough = approximatelyEqual 0.000000000001 0.000000000001 (sin pi) 0.0
```

Floating-point equality is exact and NaN is unordered. For measured or
computed quantities, `approximatelyEqual absoluteTolerance relativeTolerance`
usually expresses the intended comparison better. The two tolerances cover
different regimes: absolute tolerance matters near zero, while relative
tolerance scales with magnitude.

## Conditional expressions

Both branches must have the same type:

```solveig
absolute : Int -> Int
absolute value = if value < 0 then 0 - value else value
```

Because the whole form is an expression, it can appear inside another call:

```solveig
label : Bool -> String
label enabled = "status: " <> (if enabled then "on" else "off")
```

## Local definitions

Use `let ... in ...` when a name clarifies one expression:

```solveig
hypotenuseSquared : Int -> Int -> Int
hypotenuseSquared width height =
    let widthSquared = width * width
    in let heightSquared = height * height
    in widthSquared + heightSquared
```

Local bindings may use patterns:

```solveig
swap : forall a b. (a, b) -> (b, a)
swap pair =
    let (left, right) = pair
    in (right, left)
```

A refutable local pattern must be handled explicitly with `case`; a `let`
pattern is required to cover every value of its type.

Solveig evaluates strictly. When execution reaches a `let`, the compiler's
bytecode lowering evaluates the right-hand side and stores it before continuing
with the body:

```solveig
square : Int -> Int
square value = value * value

answer : Int
answer =
    let computed = square 21
    in computed + 1
```

`computed` is a value, not a deferred thunk. Strictness makes space use and
failure timing easier to see, but it means an unused reached binding is still
work. `if` and `case` are control flow: they evaluate the scrutinee or condition
and then only the selected branch. Use functions and streams when computation
should happen on explicit demand.

## Anonymous functions

`fun pattern => expression` creates a function:

```solveig
incrementAll : [Int] -> [Int]
incrementAll = map (fun value => value + 1)

ignore : forall a. a -> ()
ignore = fun _ => ()
```

Patterns can unpack arguments directly:

```solveig
second : forall a b. (a, b) -> b
second = fun (_, value) => value
```

Lambda parameters, top-level equations, `case` branches, and `let` bindings
share the same pattern language.

## Lists

All elements of a list have one type:

```solveig
numbers : [Int]
numbers = [10, 20, 30]

moreNumbers : [Int]
moreNumbers = 5 :: numbers
```

`[]` is empty and `head :: tail` is a non-empty list. Recursive equations make
the two cases visible:

```solveig
sumList : [Int] -> Int
sumList [] = 0
sumList (value :: rest) = value + sumList rest
```

Prefer existing combinators when they express the intent:

```solveig
positiveSquares : [Int] -> [Int]
positiveSquares values =
    map (fun value => value * value) $ filter (fun value => value > 0) values
```

Prelude supplies stack-safe `map`, `filter`, `append`, `concat`, generic strict
`foldl`, stack-safe `foldr`, and `(++)`. `std.list` adds stack-safe `take`,
`takeWhile`, `zip`, and `zipWith`, plus tail-recursive `drop`, `dropWhile`,
`reverse`, ranges, and `length`. Lists are strict finite sequences; incremental
or unbounded input uses the streaming APIs rather than an infinite List.

## Tuples

Tuples group a fixed number of possibly different types:

```solveig
person : (String, Int)
person = ("Ada", 36)

measurement : (String, Int, Bool)
measurement = ("temperature", 21, True)
```

Solveig supports tuple arities two through eight. Parentheses delimit tuple
syntax; they are not an extra layer in a signature. For example:

```solveig
measure : forall a. Task a -> Task (Int, a)
```

means “a task returning an integer and an `a`.” Nested tuples remain distinct:

```solveig
nested : (String, (Int, Bool))
nested = ("reading", (21, True))
```

Patterns preserve that structure:

```solveig
enabled : (String, (Int, Bool)) -> Bool
enabled (_, (_, flag)) = flag
```

## Pipelines and operator sections

Function composition is `(.)` and low-precedence application is `($)`:

```solveig
import std.text(trimAscii)

displayClean : String -> String
displayClean = show . trimAscii
```

Read `render . normalize` from right to left: normalize the input, then render
the result. Read `printLine $ render value` as ordinary application with the
parentheses removed. Neither operator changes evaluation strategy; both only
make the expression tree easier to read.

Operators may be used as functions:

```solveig
addOne : Int -> Int
addOne = (+) 1

prepend : forall a. a -> [a] -> [a]
prepend value = (::) value
```

Sections partially apply one side:

```solveig
greaterThanTen : Int -> Bool
greaterThanTen = (> 10)
```

Backticks let a regular function appear infix, while parentheses let an
operator appear prefix. User-defined operators declare associativity and
precedence with `infix`, `infixl`, or `infixr`.

Next: [Data, patterns, and errors](03-data-and-patterns.md).
