# Solveig

Solveig is a small typed functional language with algebraic data types, pattern matching, explicit polymorphism, higher-order functions, list syntax, and built-in integer arithmetic.

The current goal of Solveig is not “minimal toy syntax”, but a compact language with a clear functional core that can grow into a more principled language over time.

## Example

```hs
data Bool = True | False;
data Nat = Z | S Nat;
data List a = Nil | Cons a (List a);

map : forall a b. (a -> b) -> List a -> List b;
map f xs =
    case xs of
      Nil => Nil
    | Cons y ys => Cons (f y) (map f ys);

inc : Int -> Int;
inc x = x + 1;

main =
    map inc [1, 2, 3, 4];
```

## Running Solveig

At the moment, Solveig is run as a Cargo program that takes a source file and an optional entry-point name.

Typical usage:

```bash
cargo run -- path/to/program.txt
cargo run -- path/to/program.txt --entry main
cargo run -- path/to/program.txt --entry someOtherDefinition
```

The current CLI expects a file path and has an `--entry` option that defaults to `main`. The checked-in example program also uses `main` as the default entry point. 

## Language features

### 1. Algebraic data types

You can define ordinary algebraic data types with constructors.

```hs
data Maybe a = Nothing | Just a;
data Either a b = Left a | Right b;
data Nat = Z | S Nat;
data Tree a = Leaf a | Branch (Tree a) (Tree a);
```

The current example file already uses `Maybe`, `Either`, `Nat`, recursive list-like types, trees, and fixpoint-style types. 

### 2. Pattern matching

`case ... of` is the main way to inspect values.

```hs
isZero : Nat -> Bool;
isZero n =
    case n of
      Z => True
    | S _ => False;
```

Nested patterns work naturally:

```hs
maxNat : Nat -> Nat -> Nat;
maxNat x y =
    case Pair x y of
      Pair Z Z => Z
    | Pair (S a) Z => x
    | Pair Z (S b) => y
    | Pair (S a) (S b) => S (maxNat a b);
```

The current sample uses pattern matching heavily over naturals, lists, pairs, booleans, and user-defined dictionary-like values. 

### 3. Explicit polymorphism with `forall`

Solveig supports explicit universal quantification in signatures.

```hs
id : forall a. a -> a;
id x = x;

const : forall a b. a -> b -> a;
const x y = x;
```

It also supports more interesting polymorphic values, such as constructors that store polymorphic functions.

### 4. First-class and higher-order functions

Functions are ordinary values. You can pass them around, return them, and write lambdas with `fun`.

```hs
applyTwice : forall a. (a -> a) -> a -> a;
applyTwice f x = f (f x);

main =
    applyTwice (fun n => n + 1) 10;
```

The example file already contains higher-order functions such as `listMap`, `foldr`, `swap`, and manually encoded functor dictionaries. 

### 5. Lists and list syntax

Solveig supports both constructor-style lists and convenient list notation.

```hs
numbers1 = Cons 1 (Cons 2 (Cons 3 Nil));
numbers2 = [1, 2, 3];
```

You can also pattern-match using either `Nil` / `Cons` or list sugar:

```hs
sum : [Int] -> Int;
sum xs =
    case xs of
      [] => 0
    | y :: ys => y + sum ys;
```

The shipped example uses both styles. 

### 6. Conditionals

Booleans integrate with `if ... then ... else ...`.

```hs
absInt : Int -> Int;
absInt n =
    if n < 0 then -n else n;
```

The example program uses `if` in filtering logic and integer recursion. 

### 7. Primitive integers and arithmetic

Solveig supports integer literals, arithmetic, comparison, and unary minus.

```hs
distance : Int -> Int -> Int;
distance x y =
    if x >= y then x - y else y - x;

main = (2 + 2) * -10;
```

The current sample includes `Int`, arithmetic expressions, integer equality, and a recursive integer factorial.

### 8. User-defined recursion over algebraic data

A lot of Solveig code is written in the standard structural recursion style.

```hs
append : forall a. [a] -> [a] -> [a];
append xs ys =
    case xs of
      [] => ys
    | x :: rest => x :: append rest ys;
```

This style appears throughout the example file for `Nat`, lists, sorting, reversing, filtering, mapping, and folds. 

## Tutorial

A more guided introduction is available in [TUTORIAL.md](./TUTORIAL.md).

## What Solveig is good at today

Solveig already feels good for:

- small functional programs
- recursive data structures
- pattern matching
- higher-order programming
- explicit polymorphic APIs
- experimenting with typed encodings

The current example demonstrates all of those in one file.

## Current caveats

Solveig is usable, but still clearly under construction.

### Surface language is more mature than the surrounding tooling

The core language is already expressive, but the surrounding developer experience is still young. Expect rough edges in ergonomics and documentation.

### Classes and instances are not fully done yet

You already parse class/instance-related syntax in the broader project direction, but the example language style still relies on explicit dictionary encodings like:

```hs
data FunctorDict f = MkFunctor (forall a b. (a -> b) -> f a -> f b);
```

That is powerful, but still more manual than a finished type class system. The example file shows this style directly.

### `Nat` and `Int` serve different roles

`Nat` is used as an ordinary algebraic data type with constructors like `Z` and `S`, while `Int` is used for primitive arithmetic. Both appear in the current example program, and it is worth treating them as different parts of the language rather than interchangeable number types.

### The big sample file is a stress/example file, not a polished standard library

The current example source repeats several declarations and uses multiple styles in one file. That makes it useful for testing the language, but not yet a canonical “best style” program.

## Near-term roadmap

Important work that still remains:

- built-in basis / prelude cleanup for things like `Int`, `Bool`, and `Nat`
- a cleaner standard prelude
- a smaller canonical tutorial program set
- better REPL UX
- finished class / instance elaboration
- match checking improvements
- runtime polish and later GC work
- eventually effects / IO
