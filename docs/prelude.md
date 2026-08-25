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
- `prelude.function`
- `prelude.control`
- `prelude.show`
- `prelude.instances`

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
data Never
```

`Maybe` represents absence. `Either` carries one of two alternatives; by
convention, `Left` holds an expected failure and `Right` holds success.
Positional products use tuple syntax:
`(left, right)`, `(a, b, c)`, and so on through 8-tuples. The same syntax works
in types and patterns, including nesting. Tuple constructors are ordinary
values spelled `(,)`, `(,,)`, through `(,,,,,,,)`, so point-free code such as
`zipWith (,)` remains available without exposing the Prelude's internal data
constructor names.

## Typed IO and defects

```solveig
type Task a = IO HostError a

pureIO : forall e a. a -> IO e a
bindIO : forall e a b. IO e a -> (a -> IO e b) -> IO e b
failIO : forall e a. e -> IO e a
catchError : forall e f a. IO e a -> (e -> IO f a) -> IO f a
mapError : forall e f a. (e -> f) -> IO e a -> IO f a
orElse : forall e f a. IO e a -> IO f a -> IO f a
error : forall a. String -> a
```

`HostError` carries a stable `HostErrorKind`, an operation, and a diagnostic
host message. Domain modules wrap it in types such as `FsError`, `NetError`,
and `ProcessError`; strict UTF-8 decoding uses the separate `Utf8Error` type.
Defects remain distinct:

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
foldr : forall t a b. Foldable t => (a -> b -> b) -> b -> t a -> b
foldl : forall t a b. Foldable t => (b -> a -> b) -> b -> t a -> b
concat : forall a. [[a]] -> [a]
append : forall a. [a] -> [a] -> [a]
(++) : forall a. [a] -> [a] -> [a]
```

Slicing, zipping, reversal, and integer ranges are in `std.list` because many
programs never need them. `map`, `filter`, `append`, and `concat` preserve
left-to-right order and are stack-safe for user-sized finite lists. The
ordered-result operations use linear temporary List storage where a strict,
immutable singly linked representation cannot build the result in place.

## Operators and typeclasses

Ordinary operator spellings are Prelude terms, not parser-owned builtins. They
can be passed point-free, used in sections, imported, shadowed, and assigned
fixities like named functions. `(::)` is also a first-class value and resolves
to the same `Cons` constructor used by list syntax.

The comparison hierarchy separates partial and total orders:

```solveig
(==), (/=) : forall a. Eq a => a -> a -> Bool
(<), (<=), (>), (>=) : forall a. PartialOrd a => a -> a -> Bool
partialCompare : forall a. PartialOrd a => a -> a -> Maybe Ordering
compare : forall a. Ord a => a -> a -> Ordering
(+), (-), (*) : forall a. Num a => a -> a -> a
(/) : forall a. Fractional a => a -> a -> a
show : forall a. Show a => a -> String
```

`Float` has `Eq` and `PartialOrd`, but deliberately has no `Ord`: a comparison
with NaN produces `Nothing`, and ordinary relational operators return `False`.
Use `totalCompareFloat` only when sorting or another deterministic IEEE total
order is explicitly required. Signed zeroes remain equal under ordinary Float
equality and partial comparison.

`Functor`, `Applicative`, and `Monad` are universal because `do` notation
resolves through `bind` and `thenM`. Their common combinators include `fmap`,
`pure`, `apply`, `liftA2`, `join`, `composeK`, `mapM`, `sequence`, `filterM`,
`seqIO`, and `mapIO_`. Operator aliases include `<$>`, `<*>`, `<*`, `*>`,
`>>=`, and `>>`. Function composition and low-precedence application are `(.)`
and `($)`.

`Foldable` generalizes strict finite folds over `Maybe`, `List`, `Either`, and
the last element of tuples. Because Solveig is strict, `foldl` is already the
strict left fold—there is no separate `foldl'`. `foldMap` combines mapped
values through a `Monoid`.

`Traversable` visits those same containers from left to right with an
`Applicative` effect. Use `traverse transform values` or `sequenceA values`.
With `IO`, effects are deterministic and sequential; traversal never introduces
concurrency. The List implementation is stack-safe and preserves element
order.

`Hashable` provides `hashWithSalt` and `hash` for equality-respecting hashes.
Equal values always hash equally. The Prelude supplies instances for `Int`,
`Char`, `String`, `()`, `Bool`, `Ordering`, `Maybe`, finite lists, tuples, and
`Either`. `Float` intentionally has no instance because NaN is not equal to
itself. Exact numeric hashes may change between Solveig releases and must not
be serialized.

`Semigroup` and `Monoid` remain here with `combine`, `empty`, and
`mconcat`; `(<>)` is the operator alias for `combine`. They are stable
general-purpose abstractions rather than host-domain operations.

## Strings and booleans

```solveig
appendString : String -> String -> String
even : Int -> Bool
odd : Int -> Bool
not : Bool -> Bool
and : Bool -> Bool -> Bool
or : Bool -> Bool -> Bool
```

Text conversion, packed scanning, scalar-versus-byte indexing, and
`TextBuilder` are in `std.text`. UTF-8 conversion, packed byte scanning, and
`BytesBuilder` are in `std.bytes`.

## Importing explicit functionality

Import a whole module when most of its vocabulary is relevant:

```solveig
import std.io(*)
import std.task(*)
```

Prefer selective imports in modules with a narrow purpose:

```solveig
import std.list(take, zip)
import std.random(mkStdGen, randomRange)
```

An unimported std name is a normal `RES0003` unknown-name error. This is
intentional: adding a new standard-library operation cannot silently create an
ambiguity in every Solveig program.
