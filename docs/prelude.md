# Solveig Prelude Reference

The Prelude is the small, stable vocabulary automatically available to every
ordinary module. It is intentionally limited to names that almost every
program can reasonably want in scope. Domain APIs—files, processes, channels,
networking, clocks, randomness, and extended list operations—live in explicit
`std.*` modules documented in [standard-library.md](standard-library.md).
Package layout, compiler-library discovery, and the IDE's dependency edit
policy are documented in [Packages](packages.md).

Every ordinary module implicitly imports:

- `prelude.builtins`
- `prelude.list`
- `prelude.typeclasses`

`prelude.prim` is the compiler bridge used to implement the public libraries.
Application code should import `std.*`, not `prelude.prim`, unless it is
deliberately testing or extending the primitive boundary.

## Primitive types

The compiler provides these types:

```solveig
Int
Float
Char
String
Bool
()
IO error value
ThreadId
Chan value
Bytes
Socket
Handle
StdGen
```

The opaque runtime types have no numeric operations. Their values are created
and consumed through the appropriate explicit standard-library module.

## Fundamental data

```solveig
data Maybe a = Just a | Nothing
data Either a b = Left a | Right b
data Result e a = Ok a | Err e
data Pair a b = MkPair a b
data Never
```

`Maybe` represents absence. `Result` represents a pure expected failure.
`Either` carries one of two alternatives. `Pair` is the small positional
product used by APIs that return two values.

## Typed IO and defects

```solveig
type Task a = IO IOError a

pureIO : forall e a. a -> IO e a
bindIO : forall e a b. IO e a -> (a -> IO e b) -> IO e b
failIO : forall e a. e -> IO e a
catchError : forall e f a. IO e a -> (e -> IO f a) -> IO f a
mapError : forall e f a. (e -> f) -> IO e a -> IO f a
orElse : forall e f a. IO e a -> IO f a -> IO f a
error : forall a. String -> a
```

`IOError` is the closed error vocabulary produced by built-in host operations.
Its constructors stay in the Prelude because `Task` uses that type and callers
must be able to handle failures precisely. Defects remain distinct:

```solveig
data Defect =
    Error String
  | DivisionByZero
  | StackExhausted
  | HeapExhausted
```

Defect handling, cleanup, masking, and structured concurrency are explicit in
`std.task`.

Three console conveniences remain universal:

```solveig
printLine : String -> Task ()
printStr : String -> Task ()
readLine : Task String
```

## Lists

List syntax is backed by the universal data type:

```solveig
data List a = Cons a (List a) | Nil
```

The always-available operations are the common traversal vocabulary:

```solveig
map : forall a b. (a -> b) -> [a] -> [b]
filter : forall a. (a -> Bool) -> [a] -> [a]
foldr : forall a b. (a -> b -> b) -> b -> [a] -> b
foldl : forall a b. (b -> a -> b) -> b -> [a] -> b
concat : forall a. [[a]] -> [a]
append : forall a. [a] -> [a] -> [a]
(++) : forall a. [a] -> [a] -> [a]
```

Slicing, zipping, reversal, and integer ranges are in `std.list` because many
programs never need them.

## Operators and typeclasses

The Prelude defines the ordinary numeric and comparison operators through
`Eq`, `Ord`, `Num`, and `Fractional`, plus `Show` for rendering values.

```solveig
(==), (/=) : forall a. Eq a => a -> a -> Bool
(<), (<=), (>), (>=) : forall a. Ord a => a -> a -> Bool
(+), (-), (*) : forall a. Num a => a -> a -> a
(/) : forall a. Fractional a => a -> a -> a
show : forall a. Show a => a -> String
```

`Functor`, `Applicative`, and `Monad` are universal because `do` notation
resolves through `bind` and `thenM`. Their common combinators include `fmap`,
`pure`, `apply`, `liftA2`, `join`, `composeK`, `mapM`, `sequence`, `filterM`,
`seqIO`, and `mapIO_`.

`Semigroup` and `Monoid` currently remain here with `appendS`, `empty`, and
`mconcat`; they are stable general-purpose abstractions rather than host-domain
operations.

## Strings and booleans

```solveig
appendString : String -> String -> String
even : Int -> Bool
odd : Int -> Bool
not : Bool -> Bool
and : Bool -> Bool -> Bool
or : Bool -> Bool -> Bool
```

Text conversion and character-list operations are in `std.text`; byte
conversion is in `std.bytes`.

## Importing explicit functionality

Import a whole module when most of its vocabulary is relevant:

```solveig
import std.io
import std.task
```

Prefer selective imports in modules with a narrow purpose:

```solveig
import std.list(take, zip)
import std.random(mkStdGen, randomRange)
```

An unimported std name is a normal `RES0003` unknown-name error. This is
intentional: adding a new standard-library operation cannot silently create an
ambiguity in every Solveig program.
