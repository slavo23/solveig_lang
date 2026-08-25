# 2. Language basics

Solveig programs are built from expressions. An `if`, a `case`, a function,
and a `let` all produce values; they are not statement-only forms.

## Primitive values

The commonly used primitive types are:

```solveig
anInteger : Int
anInteger = 42

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

`+`, `-`, and `*` use the `Num` typeclass. `/` uses `Fractional` and therefore
works with `Float`; integer division and remainder are `div` and `mod`.

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
    map (fun value => value * value) (filter (fun value => value > 0) values)
```

Prelude supplies `map`, `filter`, generic strict `foldl`, stack-safe `foldr`,
`append`, and `(++)`.
`std.list` adds `take`, `drop`, `zip`, `zipWith`, `reverse`, ranges, and
`length`.

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
