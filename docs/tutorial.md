# Solveig Tutorial

This tutorial walks through the current Solveig language. It assumes you are
running commands from the repository root.

## 1. Create A Project

A Solveig project is a directory with `.sv` files under `src/`. The entry file
is `src/main.sv`, and its module name is `main`.

```text
hello/
`-- src/
    `-- main.sv
```

Put this in `hello/src/main.sv`:

```solveig
main : Int
main = 42
```

Run it:

```bash
cargo run -- --project-root hello
```

For a non-IO `main`, Solveig evaluates the value and displays it. If the type
has a `Show` instance, that instance controls the output.

## 2. Functions And Signatures

Top-level definitions can have explicit signatures:

```solveig
double : Int -> Int
double x = x + x

addThree : Int -> Int
addThree x = x + 3

main : Int
main = addThree (double 10)
```

Function application is written by juxtaposition: `double 10`.

Lambdas use `fun`:

```solveig
inc : Int -> Int
inc = fun x => x + 1
```

## 3. Let And If

`let` introduces a local binding:

```solveig
doublePlusOne : Int -> Int
doublePlusOne x =
    let doubled = x + x in
        doubled + 1
```

`if` is an expression:

```solveig
absInt : Int -> Int
absInt n =
    if n < 0 then
        negate n
    else
        n
```

## 4. Primitive Values

Solveig has compiler-known primitive types:

```solveig
Int
Float
Char
String
Bool
()
```

Examples:

```solveig
anInt : Int
anInt = 10

aFloat : Float
aFloat = 3.5

aChar : Char
aChar = 'x'

aString : String
aString = "hello"
```

Strings are concatenated with `appendString`:

```solveig
main : String
main = appendString "hello, " "Solveig"
```

`+` is numeric. `"a" + "b"` is rejected because there is no `Num String`
instance.

## 5. Algebraic Data Types

Define data with constructors:

```solveig
data Tree a = Leaf | Node (Tree a) a (Tree a)
```

Pattern match with `case`:

```solveig
size : forall a. Tree a -> Int
size tree =
    case tree of
        Leaf => 0
        | Node left value right => 1 + size left + size right

main : Int
main = size (Node Leaf 10 (Node Leaf 20 Leaf))
```

The checker reports non-exhaustive matches:

```solveig
bad : Bool -> Int
bad b =
    case b of
        True => 1
```

This misses `False`.

## 6. Lists

Lists have literal syntax:

```solveig
numbers : [Int]
numbers = [1, 2, 3, 4]
```

Use `[]` and `::` in patterns:

```solveig
sum : [Int] -> Int
sum xs =
    case xs of
        [] => 0
        | x :: rest => x + sum rest
```

Useful Prelude functions:

```solveig
map : forall a b. (a -> b) -> [a] -> [b]
filter : forall a. (a -> Bool) -> [a] -> [a]
foldl : forall a b. (b -> a -> b) -> b -> [a] -> b
foldr : forall a b. (a -> b -> b) -> b -> [a] -> b
reverse : forall a. [a] -> [a]
range : Int -> Int -> Bool -> [Int]
```

Example:

```solveig
main : [Int]
main = map (fun x => x * x) (range 1 5 True)
```

prints:

```text
[1, 4, 9, 16, 25]
```

## 7. Modules And Imports

File paths determine module names:

```text
src/main.sv          -> main
src/math/stats.sv    -> math.stats
```

Example project:

```text
stats-demo/
`-- src/
    |-- main.sv
    `-- math/
        `-- stats.sv
```

`src/math/stats.sv`:

```solveig
sum : [Int] -> Int
sum xs =
    case xs of
        [] => 0
        | x :: rest => x + sum rest
```

`src/main.sv`:

```solveig
import math.stats(sum)

main : Int
main = sum [1, 2, 3]
```

Imports can be whole-module, selective, or aliased:

```solveig
import math.stats
import math.stats(sum)
import math.stats as stats
```

## 8. Polymorphism

Use `forall` for explicit polymorphism:

```solveig
id : forall a. a -> a
id x = x

const : forall a b. a -> b -> a
const x y = x
```

Higher-rank annotations are supported:

```solveig
usePoly : (forall a. a -> a) -> Pair Int Bool
usePoly f = MkPair (f 1) (f True)
```

## 9. Typeclasses

Typeclasses are single-parameter and can have superclasses.

`Eq` provides equality:

```solveig
same : forall a. Eq a => a -> a -> Bool
same x y = x == y
```

`Ord` provides comparison:

```solveig
smallest : forall a. Ord a => a -> a -> a
smallest x y = if x <= y then x else y
```

`Num` provides numeric operators:

```solveig
square : forall a. Num a => a -> a
square x = x * x
```

Define instances for your own types:

```solveig
data Direction = North | South

instance Eq Direction where
    (==) left right =
        case MkPair left right of
            MkPair North North => True
            | MkPair South South => True
            | MkPair _ _ => False
```

## 10. Show And Value Display

`Show` converts values to strings:

```solveig
class Show a where
    show : a -> String
```

When `main` is not an IO action, Solveig uses `Show` if an instance exists:

```solveig
main : Maybe Int
main = Just 10
```

prints:

```text
Just 10
```

Add `Show` for your own type:

```solveig
data Person = Person String Int

instance Show Person where
    show person =
        case person of
            Person name age =>
                appendString name (appendString " is " (show age))

main : Person
main = Person "Ada" 36
```

## 11. Operators And Fixity

Symbolic operators are ordinary functions and can have fixity declarations:

```solveig
infixl 6 <+>

(<+>) : Int -> Int -> Int
(<+>) x y = x + y + 1

main : Int
main = 1 <+> 2 <+> 3
```

Operators without declarations default to `infixl 9`. Operators starting with
`:` are reserved for future constructor operators. The list cons operator `::`
is special syntax today.

Parenthesize mixed operators when fixities would be ambiguous:

```solveig
main = (a <+> b) <*> c
```

## 12. Do Notation And Monadic IO

An executable IO program has `main : IO ()` or, more generally, `main : IO a`.
IO programs print explicitly; Solveig does not print the returned value of an
IO `main`.

```solveig
main : IO ()
main = printLine "hello from IO"
```

Sequence actions with `do`:

```solveig
main : IO ()
main = do
    printStr "name> "
    name <- readLine
    printLine (appendString "hello, " name)
```

`do` notation is not IO-only. It desugars through the generic `Monad` class:
`x <- action` uses `bind`, ordinary expression statements use `thenM`, and the
last statement is the result expression.

```solveig
maybeAnswer : Maybe Int
maybeAnswer = do
    x <- Just 2
    y <- Just 40
    pure (x + y)
```

The first version supports variable binds, `_ <- action`, local
`let name = expr` statements, and ordinary expression statements. Pattern
binds are reserved for later.

Run it:

```bash
cargo run -- --project-root hello
```

Pass program args after `--`:

```solveig
main : IO ()
main = do
    args <- getArgs
    mapIO_ printLine args
```

```bash
cargo run -- --project-root args-demo -- first second
```

## 13. File IO

Text file IO is in the Prelude:

```solveig
readFile : String -> IO String
writeFile : String -> String -> IO ()
appendFile : String -> String -> IO ()
```

Relative paths are resolved against the project root.

```solveig
main : IO ()
main = do
    contents <- readFile "input.txt"
    printLine contents
    writeFile "copy.txt" contents
```

The repository includes a richer example:

```bash
cargo run -- --project-root examples/quiz_game
```

It reads `questions.txt`, prompts for answers, prints a score, writes
`last-score.txt`, and appends `score-history.txt`.

## 14. Diagnostics

Diagnostics include stable phase-prefixed codes such as `PAR0001`, `RES0003`,
`KIND0004`, and `TC0013`.

For example:

```solveig
main : String
main = "left" + "right"
```

reports `TC0013` because the `+` operator requires `Num String`.

See [diagnostics.md](diagnostics.md) for the full code index.

## 15. Where To Go Next

- Read [prelude.md](prelude.md) for signatures and Prelude examples.
- Browse `tests/golden/run/` for small executable programs.
- Browse `examples/smoke_suite` for larger algorithm examples.
- Browse `examples/quiz_game` for practical IO and a local `State` monad.
