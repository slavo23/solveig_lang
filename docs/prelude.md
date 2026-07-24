# Solveig Prelude Reference

Every ordinary module implicitly imports:

- `prelude.builtins`
- `prelude.list`
- `prelude.typeclasses`

The compiler also provides `prelude.prim`, a low-level primitive module. It is
not imported implicitly; user programs usually prefer the wrappers and classes
from the ordinary Prelude.

## Primitive Types

These types are compiler-known:

```solveig
Int
Float
Char
String
IO a
Bool
()
```

`Int` is a signed 64-bit integer. `Float` is an IEEE-754 `f64`. `String` is
immutable UTF-8 text. `IO a` is an abstract monadic action.

## `prelude.builtins`

### Data Types

```solveig
data Maybe a = Just a | Nothing
data Either a b = Left a | Right b
data Pair a b = MkPair a b
```

`Bool` is compiler-defined and exported as:

```solveig
True : Bool
False : Bool
```

### IO

```solveig
pureIO : forall a. a -> IO a
bindIO : forall a b. IO a -> (a -> IO b) -> IO b
printLine : String -> IO ()
printStr : String -> IO ()
readLine : IO String
readFile : String -> IO String
writeFile : String -> String -> IO ()
appendFile : String -> String -> IO ()
```

`printLine` writes a trailing newline. `printStr` does not, and the CLI flushes
after each print so prompts appear before `readLine`.

`readLine` returns a line without the trailing newline. At EOF it returns `""`.

`readFile`, `writeFile`, and `appendFile` are whole-file UTF-8 text operations.
Relative paths are resolved against the Solveig project root.

Example:

```solveig
main : IO ()
main =
    seqIO
        (printStr "name> ")
        (bindIO readLine (fun name =>
            printLine (appendString "hello, " name)))
```

### Strings And Booleans

```solveig
appendString : String -> String -> String

even : Int -> Bool
odd : Int -> Bool
not : Bool -> Bool
and : Bool -> Bool -> Bool
or : Bool -> Bool -> Bool
```

Use `appendString` for ordinary string concatenation. The `+` operator belongs
to `Num`; there is no `Num String` instance.

## `prelude.list`

### Data Type

```solveig
data List a = Cons a (List a) | Nil
```

Lists can also be written with surface syntax:

```solveig
[] : [a]
x :: xs : [a]
[1, 2, 3] : [Int]
```

### Functions

```solveig
map : forall a b. (a -> b) -> [a] -> [b]
filter : forall a. (a -> Bool) -> [a] -> [a]
foldr : forall a b. (a -> b -> b) -> b -> [a] -> b
foldl : forall a b. (b -> a -> b) -> b -> [a] -> b
concat : forall a. [[a]] -> [a]
append : forall a. [a] -> [a] -> [a]
reverse : forall a. [a] -> [a]
range : Int -> Int -> Bool -> [Int]
```

`range start end inclusive` counts upward. When `inclusive` is `True`, `end`
is included; otherwise it stops before `end`.

Examples:

```solveig
squares : [Int]
squares = map (fun x => x * x) [1, 2, 3, 4]

sum : [Int] -> Int
sum xs = foldl (fun total x => total + x) 0 xs

firstFive : [Int]
firstFive = range 1 5 True
```

## `prelude.typeclasses`

### Ordering Data

```solveig
data Ordering = LT | EQ | GT
```

### Equality

```solveig
class Eq a where
    (==) : a -> a -> Bool
    (/=) : a -> a -> Bool

eq : forall a. Eq a => a -> a -> Bool
notEq : forall a. Eq a => a -> a -> Bool
```

Instances are provided for `Int`, `Float`, `Char`, `String`, `()`, `Bool`,
`Maybe a`, `[a]`, and `Pair a b` when their element types have `Eq`.

### Ord

```solveig
class Eq a => Ord a where
    compare : a -> a -> Ordering
    (<) : a -> a -> Bool
    (<=) : a -> a -> Bool
    (>) : a -> a -> Bool
    (>=) : a -> a -> Bool

less : forall a. Ord a => a -> a -> Bool
lessEq : forall a. Ord a => a -> a -> Bool
greater : forall a. Ord a => a -> a -> Bool
greaterEq : forall a. Ord a => a -> a -> Bool
minOrd : forall a. Ord a => a -> a -> a
maxOrd : forall a. Ord a => a -> a -> a
clampOrd : forall a. Ord a => a -> a -> a -> a
```

Instances are provided for `Int`, `Float`, `Char`, `String`, `()`, `Bool`,
`Maybe a`, `[a]`, and `Pair a b` when their element types have `Ord`.

### Numeric Classes

```solveig
class Num a where
    (+) : a -> a -> a
    (-) : a -> a -> a
    (*) : a -> a -> a
    negate : a -> a

div : Int -> Int -> Int
mod : Int -> Int -> Int

class Num a => Fractional a where
    (/) : a -> a -> a
```

`Num` instances exist for `Int` and `Float`. `Fractional` currently has only a
`Float` instance. Int division uses `div`; `/` is not Int division.

### Functor, Applicative, Monad

```solveig
class Functor f where
    fmap : forall a b. (a -> b) -> f a -> f b

class Functor f => Applicative f where
    pure : forall a. a -> f a
    apply : forall a b. f (a -> b) -> f a -> f b

liftA2 : forall f a b c. Applicative f => (a -> b -> c) -> f a -> f b -> f c
applyRight : forall f a b. Applicative f => f a -> f b -> f b
applyLeft : forall f a b. Applicative f => f a -> f b -> f a

class Applicative m => Monad m where
    bind : forall a b. m a -> (a -> m b) -> m b

thenM : forall m a b. Monad m => m a -> m b -> m b
join : forall m a. Monad m => m (m a) -> m a
composeK : forall m a b c. Monad m => (a -> m b) -> (b -> m c) -> a -> m c
mapM : forall m a b. Monad m => (a -> m b) -> [a] -> m [b]
sequence : forall m a. Monad m => [m a] -> m [a]
filterM : forall m a. Monad m => (a -> m Bool) -> [a] -> m [a]
```

Instances are provided for `Maybe`, `List`, and `IO` where appropriate.

Example:

```solveig
readTwo : IO [String]
readTwo = mapM (fun prompt =>
    seqIO (printStr prompt) readLine) ["first> ", "second> "]
```

### IO Helpers

```solveig
getArgs : IO [String]
seqIO : forall a b. IO a -> IO b -> IO b
mapIO_ : forall a. (a -> IO ()) -> [a] -> IO ()
```

`seqIO left right` runs `left`, discards its result, then runs `right`.

`mapIO_ f xs` runs an IO action for each list element and discards all results.

Example:

```solveig
main : IO ()
main = bindIO getArgs (fun args => mapIO_ printLine args)
```

### Show

```solveig
class Show a where
    show : a -> String
```

Instances are provided for `Int`, `Float`, `Char`, `String`, `()`, `Bool`,
`Ordering`, `Maybe a`, `[a]`, and `Pair a b` when their element types have
`Show`.

For non-IO `main`, the driver uses `Show` when possible:

```solveig
main : [Int]
main = [1, 2, 3]
```

prints:

```text
[1, 2, 3]
```

User types can opt in:

```solveig
data Person = Person String Int

instance Show Person where
    show person =
        case person of
            Person name age =>
                appendString name (appendString " (" (appendString (show age) ")"))
```

### Semigroup And Monoid

```solveig
class Semigroup a where
    appendS : a -> a -> a

class Semigroup a => Monoid a where
    empty : a

mconcat : forall a. Monoid a => [a] -> a
```

The classes are present, but the Prelude currently does not ship broad
instances for them.

## `prelude.prim`

`prelude.prim` is the low-level primitive bridge. Import it explicitly when
you need a primitive not wrapped by the public Prelude:

```solveig
import prelude.prim(stringLength, stringSlice, stringCharAt)
```

Primitive functions include:

```solveig
intAdd : Int -> Int -> Int
intSub : Int -> Int -> Int
intMul : Int -> Int -> Int
intDiv : Int -> Int -> Int
intMod : Int -> Int -> Int
intNeg : Int -> Int
intEq : Int -> Int -> Bool
intLt : Int -> Int -> Bool
intLe : Int -> Int -> Bool
intGt : Int -> Int -> Bool
intGe : Int -> Int -> Bool
intCompare : Int -> Int -> Ordering

floatAdd : Float -> Float -> Float
floatSub : Float -> Float -> Float
floatMul : Float -> Float -> Float
floatDiv : Float -> Float -> Float
floatNeg : Float -> Float
floatEq : Float -> Float -> Bool
floatLt : Float -> Float -> Bool
floatLe : Float -> Float -> Bool
floatGt : Float -> Float -> Bool
floatGe : Float -> Float -> Bool

intToFloat : Int -> Float
floatToInt : Float -> Int
intToString : Int -> String
floatToString : Float -> String
charToString : Char -> String

stringLength : String -> Int
stringAppend : String -> String -> String
stringSlice : String -> Int -> Int -> String
stringCharAt : String -> Int -> Maybe Char
stringEq : String -> String -> Bool
stringCompare : String -> String -> Ordering
stringToChars : String -> [Char]
charsToString : [Char] -> String
charEq : Char -> Char -> Bool
charCompare : Char -> Char -> Ordering

pureIO : forall a. a -> IO a
bindIO : forall a b. IO a -> (a -> IO b) -> IO b
printLine : String -> IO ()
printStr : String -> IO ()
readLine : IO String
getArgs : IO [String]
readFile : String -> IO String
writeFile : String -> String -> IO ()
appendFile : String -> String -> IO ()
```

Primitive Int add/sub/mul/neg wrap on `i64` overflow. `intDiv` and `intMod`
report runtime errors on division/modulo by zero. String indexing and slicing
count Unicode scalar values.
