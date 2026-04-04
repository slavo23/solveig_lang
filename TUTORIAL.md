# Solveig Tutorial

This tutorial is a user-facing introduction to writing small Solveig programs.

It focuses on the surface language: defining data, writing functions, using pattern matching, and combining polymorphism with higher-order programming.

## 1. Your first definition

A simple top-level definition can be just a value:

```hs
main = 42;
```

If you run the file with `main` as the entry point, Solveig evaluates that definition and prints the result.

The current CLI supports selecting an entry definition with `--entry`, and defaults to `main`.

## 2. Functions

Functions are curried by default.

```hs
add : Int -> Int -> Int;
add x y = x + y;

main = add 20 22;
```

Anonymous functions use `fun`:

```hs
main = (fun x => x + 1) 41;
```

You can annotate lambda parameters when useful:

```hs
main = (fun (x : Int) => x * 2) 21;
```

## 3. Booleans and conditionals

Booleans are ordinary values:

```hs
data Bool = True | False;
```

Conditionals look like this:

```hs
absInt : Int -> Int;
absInt n =
    if n < 0 then -n else n;

main = absInt -12;
```

The example program uses this same style for filtering and integer recursion. 

## 4. Algebraic data types

You can define your own data.

```hs
data Maybe a = Nothing | Just a;
data Pair a b = Pair a b;
data Nat = Z | S Nat;
```

Constructors are used like ordinary functions:

```hs
pair = Pair 4 2;
one = S Z;
two = S one;
```

The project example uses all of these patterns already.

## 5. Pattern matching

Pattern matching is the central elimination form.

```hs
not : Bool -> Bool;
not b =
    case b of
      True => False
    | False => True;
```

A classic `Maybe` example:

```hs
withDefault : forall a. a -> Maybe a -> a;
withDefault fallback mx =
    case mx of
      Nothing => fallback
    | Just x => x;
```

And recursion over naturals:

```hs
isZero : Nat -> Bool;
isZero n =
    case n of
      Z => True
    | S _ => False;
```

## 6. Lists

Solveig supports both explicit constructors and list sugar.

```hs
data List a = Nil | Cons a (List a);
```

These two definitions mean the same thing:

```hs
xs1 = Cons 1 (Cons 2 (Cons 3 Nil));
xs2 = [1, 2, 3];
```

Pattern matching also has sugared list forms:

```hs
headOrZero : [Int] -> Int;
headOrZero xs =
    case xs of
      [] => 0
    | y :: ys => y;
```

The example file uses both `Cons` / `Nil` and `[]` / `::`.

## 7. Recursion over lists

A standard recursive function:

```hs
length : forall a. [a] -> Int;
length xs =
    case xs of
      [] => 0
    | _ :: ys => 1 + length ys;
```

Another common one:

```hs
map : forall a b. (a -> b) -> [a] -> [b];
map f xs =
    case xs of
      [] => []
    | y :: ys => f y :: map f ys;
```

The current example already contains analogous definitions such as `listMap`, `filter`, `append`, `reverse`, and `foldr`. 

## 8. Explicit polymorphism

Solveig makes polymorphism visible in signatures:

```hs
id : forall a. a -> a;
id x = x;

const : forall a b. a -> b -> a;
const x y = x;
```

This is often a good thing: the API surface stays explicit and readable.

You can then instantiate those definitions in ordinary code:

```hs
main = [id 1, id 2, id 3];
```

The sample program also goes further and stores polymorphic functions inside constructors.

## 9. Higher-order functions

Functions are ordinary values, so higher-order programming is natural.

```hs
compose : forall a b c. (b -> c) -> (a -> b) -> a -> c;
compose f g x = f (g x);

twice : forall a. (a -> a) -> a -> a;
twice f x = f (f x);

main = twice (fun n => n + 10) 5;
```

This style matches the example file’s `foldr`, `listMap`, `swap`, and dictionary encodings.

## 10. `Nat` versus `Int`

Solveig currently has two useful number stories.

### `Nat`

Use `Nat` when you want a user-defined inductive structure:

```hs
data Nat = Z | S Nat;

addNat : Nat -> Nat -> Nat;
addNat n m =
    case n of
      Z => m
    | S k => S (addNat k m);
```

### `Int`

Use `Int` when you want built-in arithmetic:

```hs
fact : Int -> Int;
fact n =
    if n == 0 then 1 else n * fact (n - 1);
```

Both appear in the current example, and they serve different purposes.

## 11. A small complete example

```hs
data Bool = True | False;
data Maybe a = Nothing | Just a;

safeDiv : Int -> Int -> Maybe Int;
safeDiv x y =
    if y == 0 then Nothing else Just (x / y);

showResult : Maybe Int -> Int;
showResult mx =
    case mx of
      Nothing => 0
    | Just n => n;

main = showResult (safeDiv 20 5);
```

This program shows:

- algebraic data types
- `if`
- pattern matching
- primitive integer arithmetic
- ordinary top-level definitions

## 12. A small list-processing example

```hs
keepPositive : [Int] -> [Int];
keepPositive xs =
    case xs of
      [] => []
    | y :: ys =>
        if y > 0 then y :: keepPositive ys
        else keepPositive ys;

doubleAll : [Int] -> [Int];
doubleAll xs =
    case xs of
      [] => []
    | y :: ys => (y * 2) :: doubleAll ys;

main = doubleAll (keepPositive [-2, 7, 0, 9, -1, 5]);
```

## 13. Current caveats

Solveig is already expressive, but it is still evolving.

A few practical notes:

- the large sample source is more of a feature/demo file than a polished tutorial source
- some areas of the language are still settling, especially around builtins, prelude structure, and future class elaboration
- the language is ahead of the surrounding ergonomics right now

The example file itself reflects that exploratory state: it mixes multiple examples and repeats some declarations.

## 14. What to learn next

After this tutorial, the next useful things to try are:

1. write your own `Maybe` utilities
2. write list functions with `[]` and `::`
3. write `Nat` functions using structural recursion
4. write higher-order utilities like `map`, `fold`, or `compose`
5. experiment with explicit `forall` signatures
