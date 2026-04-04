# Solveig

Solveig is a small statically typed functional language under active development.

Its current design centers on a compact core with:

- algebraic data types
- pattern matching
- first-class functions
- explicit universal quantification
- higher-rank polymorphism
- list syntax
- local `let` bindings
- built-in integer arithmetic and comparisons

The goal is not to imitate a large production language in every detail, but to provide a serious, principled functional language core that is expressive enough for non-trivial programs and precise enough to serve as a foundation for further work.



## Status

Solveig already supports a meaningful fragment of typed functional programming.

The current example programs demonstrate:

- recursive algebraic data types such as `List`, `Maybe`, `Either`, `Nat`, `Tree`, and `Fix`
- parametric polymorphism with explicit `forall`
- higher-order programming with functions such as `map`, `filter`, `foldr`, and `foldl`
- higher-rank polymorphism, including values and fields whose types themselves contain `forall`
- local `let ... in ...`
- list literals and `::` patterns
- primitive integer programs using `Int`, arithmetic, and comparisons

See the attached example programs for representative code.


## Building and running

Solveig is currently built and run through Cargo.

The command-line driver accepts:

- a source file path
- an optional entry-point name
- default entry point: `main` 

### Run directly

```bash
cargo run -- path/to/program.txt
cargo run -- path/to/program.txt --entry main
cargo run -- path/to/program.txt -e someOtherDefinition
```

### Build a release binary

```bash
cargo build --release
```

Then run the produced executable:

```bash
./target/release/<binary-name> path/to/program.solveig --entry main
```

The binary name depends on the Cargo package name.

---

## Example

```hs
data Bool = True | False;
data List a = Nil | Cons a (List a);

map : forall a b. (a -> b) -> [a] -> [b];
map f xs =
    case xs of
        [] => []
        | y :: ys => (f y) :: map f ys;

inc : Int -> Int;
inc x = x + 1;

main = map inc [1, 2, 3, 4];
```

This example already shows several core aspects of the language:

- algebraic data
- explicit polymorphism
- list syntax
- pattern matching
- higher-order programming
- primitive integer arithmetic


## Core language features

## Algebraic data types

Solveig supports user-defined algebraic data types.

```hs
data Maybe a = Nothing | Just a;
data Either a b = Left a | Right b;
data Nat = Z | S Nat;
data List a = Nil | Cons a (List a);
```

Recursive and parameterized definitions are both supported. The current larger example also includes types such as `Tree`, `Forest`, `Compose`, and `Fix`. 

## Pattern matching

Pattern matching is the primary elimination form for algebraic data.

```hs
isZero : Nat -> Bool;
isZero n =
    case n of
        Z => True
        | S _ => False;
```

Patterns are also used with lists:

```hs
sum : [Int] -> Int;
sum xs =
    case xs of
        [] => 0
        | y :: ys => y + sum ys;
```

## Functions and higher-order programming

Functions are first-class and curried.

```hs
id : forall a. a -> a;
id x = x;

applyTwice : forall a. (a -> a) -> a -> a;
applyTwice f x = f (f x);
```

Anonymous functions use `fun`:

```hs
main = applyTwice (fun n => n + 1) 10;
```

The current examples already make extensive use of higher-order programming via `map`, `filter`, `foldr`, `foldl`, and function-valued records encoded as data. turn16file0 

## Explicit polymorphism

Solveig uses explicit `forall` in source types.

```hs
id : forall a. a -> a;
id x = x;

const : forall a b. a -> b -> a;
const x y = x;
```

This is a deliberate part of the language style: polymorphism is visible in the source language rather than hidden behind fully implicit syntax.

## Higher-rank polymorphism

Solveig already supports higher-rank types, not just ordinary rank-1 top-level polymorphism.

Representative examples from the current codebase include:

```hs
data Id = MkId (forall a. a -> a);

testPolyMatch : Nat -> (forall a. a -> a);

data FunctorDict f = MkFunctor (forall a b. (a -> b) -> f a -> f b);
```

These are genuine higher-rank uses: polymorphic functions appear as constructor fields and as function results, not only at the outermost type level. 

## Lists

Solveig supports both constructor-style and sugared list syntax.

```hs
xs1 = Cons 1 (Cons 2 (Cons 3 Nil));
xs2 = [1, 2, 3];
```

List patterns are also available:

```hs
headOrZero : [Int] -> Int;
headOrZero xs =
    case xs of
        [] => 0
        | y :: ys => y;
```

## Conditionals

```hs
absInt : Int -> Int;
absInt n =
    if n < 0 then -n else n;
```

This is used throughout the examples for boolean control flow and integer recursion. 

## Local bindings

Solveig supports local `let ... in ...` bindings.

```hs
sumList =
    let add = fun x y => x + y
    in foldl add 1;
```

This is present directly in the smaller example program. turn16file0

## Integers and arithmetic

`Int` is available for ordinary arithmetic and comparisons.

```hs
distance : Int -> Int -> Int;
distance x y =
    if x >= y then x - y else y - x;
```

The current examples also include integer range construction, arithmetic parsing, and recursive integer factorial.


## Brief tutorial

## 1. Define data

```hs
data Bool = True | False;
data Maybe a = Nothing | Just a;
data List a = Nil | Cons a (List a);
```

## 2. Define typed functions

```hs
not : Bool -> Bool;
not b =
    case b of
        True => False
        | False => True;

id : forall a. a -> a;
id x = x;
```

## 3. Write recursive functions by pattern matching

```hs
map : forall a b. (a -> b) -> [a] -> [b];
map f xs =
    case xs of
        [] => []
        | y :: ys => (f y) :: map f ys;
```

```hs
foldr : forall a b. (a -> b -> b) -> b -> [a] -> b;
foldr f z xs =
    case xs of
        [] => z
        | y :: ys => f y (foldr f z ys);
```

The current examples also include `foldl`, `filter`, `append`, `reverse`, and list-based sorting.

## 4. Use lambdas when local function values are clearer

```hs
main = map (fun x => x + 1) [1, 2, 3, 4];
```

## 5. Use local let-bindings for structure

```hs
sum : [Int] -> Int;
sum xs =
    let add = fun acc x => acc + x
    in foldl add 0 xs;
```

## 6. Use `main` or another selected entry point

```hs
main = foldl (fun acc x => acc + x) 0 [1, 2, 3, 4];
```

By default, Solveig runs `main`, but another top-level definition can be chosen with `--entry`. 

## Repository examples

Two attached examples are especially useful:

### `with_prelude.txt`

A smaller program showing:

- `filter`
- `map`
- `foldr`
- `foldl`
- `intRange`
- local `let`
- integer list processing

### `main.txt`

A larger feature file showing:

- a broader collection of algebraic data types
- `Nat`-based recursion
- list processing
- higher-order programming
- higher-rank polymorphism
- explicit dictionary encodings
- both `Nat` and `Int` programs 

---

## Notes

- `Nat` and `Int` play different roles in the current examples: `Nat` is an ordinary algebraic type, while `Int` is used for primitive arithmetic. 
- the larger example file is intentionally broad and exploratory; it is better read as a feature exercise than as a canonical standard library
