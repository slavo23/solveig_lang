# 4. Functions, modules, and typeclasses

Solveig's abstraction tools are small but composable: parametric polymorphism,
higher-order functions, modules, type aliases, typeclasses, and instances.

## Polymorphic functions

Lowercase names in a `forall` are type variables:

```solveig
identity : forall a. a -> a
identity value = value

pairWith : forall a b. a -> b -> (a, b)
pairWith left right = (left, right)
```

The implementation cannot inspect an unknown `a`; it must work uniformly for
every type selected by the caller. This restriction is useful: the signature
rules out whole classes of behavior.

## Higher-order functions

Functions are values and may be passed or returned:

```solveig
twice : forall a. (a -> a) -> a -> a
twice transform value = transform (transform value)

add : Int -> Int -> Int
add left right = left + right

addTen : Int -> Int
addTen = add 10
```

Application associates left, while arrows associate right. Therefore `add 10
2` means `(add 10) 2`, and `Int -> Int -> Int` means `Int -> (Int -> Int)`.

## Type aliases

Aliases give a domain name to an existing type:

```solveig
type UserId = Int
type Handler a = String -> Task a
```

An alias does not create a new runtime representation or a distinct nominal
type. Use `data` when values must be kept separate by the typechecker:

```solveig
data UserId = UserId Int
```

Aliases may take parameters, but every use must supply all of the alias's type
arguments. If higher-kinded partial application is required, define a real
`data` type instead.

## Modules

Every `.sv` file below `src/` is a module. For package `shop`,
`src/order/price.sv` has the fully qualified name `shop.order.price`.

Inside the same package, import it relatively:

```solveig
import order.price(total)
```

Select names to keep the local namespace small, or import the module and use a
qualified reference:

```solveig
import std.list

firstThree : forall a. [a] -> [a]
firstThree = std.list.take 3
```

When two unqualified imports provide the same name, Solveig reports ambiguity
instead of silently selecting one.

## Typeclasses

A typeclass describes operations shared by multiple types:

```solveig
class Render a where
    render : a -> String
```

An instance supplies the behavior:

```solveig
data Priority = Low | High

instance Render Priority where
    render Low = "low"
    render High = "high"
```

Instance methods support adjacent pattern-matching equations, just like normal
functions. A constrained function asks for the capability it needs:

```solveig
renderTwice : forall a. Render a => a -> String
renderTwice value = render value <> ", " <> render value
```

Common Prelude classes include:

- `Eq` for equality;
- `PartialOrd` for values that may be incomparable, including `Float`;
- `Ord` for total ordering;
- `Num` and `Fractional` for numeric operations;
- `Show` for programmer-oriented text;
- `Semigroup` and `Monoid` for combination;
- `Functor`, `Applicative`, and `Monad` for structured computation.

## Functor, Applicative, and Monad

`fmap` transforms a value inside a context without changing the context:

```solveig
incrementMaybe : Maybe Int -> Maybe Int
incrementMaybe = fmap (fun value => value + 1)
```

`pure` introduces a value, and `apply` combines contextual functions and
values. `bind` sequences a computation whose next step depends on its result.
The operators are `<$>`, `<*>`, and `>>=`.

`do` notation is the readable form of monadic sequencing:

```solveig
readAndPrint : Task ()
readAndPrint = do
    line <- readLine
    printLine line
```

It is not specific to IO; any `Monad` instance can use the same notation.

## Point-free style

A definition may omit an argument when composition is clearer:

```solveig
import std.text(trimAscii)

cleanDisplay : String -> String
cleanDisplay = show . trimAscii
```

Operators and ordinary names are interchangeable in prefix/infix position:

```solveig
appendOne : [Int] -> [Int]
appendOne = flip (++) [1]

containsAnswer : [Int] -> [Int]
containsAnswer = filter (== 42)
```

Use point-free code when it exposes a pipeline. Name the argument when doing so
makes control flow, error handling, or pattern matching easier to follow.

## Higher-rank arguments

Solveig supports explicit `forall` inside signatures. Parentheses mark the
scope when a function receives a polymorphic function:

```solveig
useIdentity : (forall a. a -> a) -> (Int, String)
useIdentity polymorphic = (polymorphic 7, polymorphic "seven")
```

Higher-rank code normally needs a written signature because inference cannot
guess where the inner quantifier belongs.

Next: [IO and files](05-io-and-files.md).
