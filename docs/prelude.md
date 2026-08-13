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
IO e a
Bool
()
```

`Int` is a signed 64-bit integer. `Float` is an IEEE-754 `f64`. `String` is
immutable UTF-8 text. `IO e a` is an abstract monadic action that may fail with
`e` or succeed with `a`.

See [Errors, Defects, and Interruption](errors.md) for the semantic distinction
between typed errors, defects, interruption, and VM infrastructure failures.

## `prelude.builtins`

### Data Types

```solveig
data Maybe a = Just a | Nothing
data Either a b = Left a | Right b
data Pair a b = MkPair a b
data Result e a = Ok a | Err e
data Never
data IOError =
    StdoutError String
  | StdinError String
  | ReadFileError String String
  | WriteFileError String String
  | AppendFileError String String
  | ChannelClosed
  | InvalidUtf8 String
  | ResolveHostError String
  | ConnectError String
  | ListenError String
  | AcceptError String
  | SendError String
  | RecvError String
  | CloseSocketError String
  | OpenHandleError String String
  | CloseHandleError String
  | FlushHandleError String
  | ReadHandleError String
  | WriteHandleError String
  | SetBufferingError String
data IOMode = ReadMode | WriteMode | AppendMode | ReadWriteMode
data BufferMode = NoBuffering | LineBuffering | BlockBuffering (Maybe Int)
data Defect =
    Error String
  | DivisionByZero
  | StackExhausted
  | HeapExhausted
type Task a = IO IOError a
```

`Bool` is compiler-defined and exported as:

```solveig
True : Bool
False : Bool
```

### IO

```solveig
pureIO : forall e a. a -> IO e a
bindIO : forall e a b. IO e a -> (a -> IO e b) -> IO e b
failIO : forall e a. e -> IO e a
error : forall a. String -> a
catchError : forall e f a. IO e a -> (e -> IO f a) -> IO f a
catchDefect : forall e a. IO e a -> (Defect -> IO e a) -> IO e a
mapError : forall e f a. (e -> f) -> IO e a -> IO f a
orElse : forall e f a. IO e a -> IO f a -> IO f a
finally : forall e a b. IO e a -> IO e b -> IO e a
bracket : forall e resource cleanup a. IO e resource -> (resource -> IO e cleanup) -> (resource -> IO e a) -> IO e a
withThread : forall e a b. IO e a -> (ThreadId -> IO e b) -> IO e b
race : forall e a b. IO e a -> IO e b -> IO e (Either a b)
concurrently : forall e a b. IO e a -> IO e b -> IO e (Pair a b)
timeout : forall e a. Int -> IO e a -> IO e (Maybe a)
printLine : String -> Task ()
printStr : String -> Task ()
readLine : Task String
stdin : Handle
stdout : Handle
stderr : Handle
openFile : String -> IOMode -> Task Handle
hClose : Handle -> Task ()
hFlush : Handle -> Task ()
hGetLine : Handle -> Task String
hGetContents : Handle -> Task String
hPutStr : Handle -> String -> Task ()
hPutStrLn : Handle -> String -> Task ()
hIsEOF : Handle -> Task Bool
hSetBuffering : Handle -> BufferMode -> Task ()
withFile : forall a. String -> IOMode -> (Handle -> Task a) -> Task a
readFile : String -> Task String
writeFile : String -> String -> Task ()
appendFile : String -> String -> Task ()
spawn : forall childError e. IO childError () -> IO e ThreadId
killThread : ThreadId -> Task ()
sleep : Int -> Task ()
mask : forall e a. IO e a -> IO e a
newChan : forall e a. IO e (Chan a)
newBoundedChan : forall e a. Int -> IO e (Chan a)
send : forall a. Chan a -> a -> Task ()
recv : forall a. Chan a -> Task a
tryRecv : forall a. Chan a -> Task (Maybe a)
closeChan : forall a. Chan a -> Task ()
stringToBytes : String -> Bytes
bytesToString : Bytes -> Result IOError String
bytesLength : Bytes -> Int
bytesIndex : Bytes -> Int -> Maybe Int
bytesSlice : Bytes -> Int -> Int -> Bytes
bytesAppend : Bytes -> Bytes -> Bytes
resolveHost : String -> Task String
connect : String -> Int -> Task Socket
listen : Int -> Task (Pair Socket Int)
accept : Socket -> Task Socket
sendBytes : Socket -> Bytes -> Task ()
recvBytes : Socket -> Int -> Task Bytes
closeSocket : Socket -> Task ()
```

`printLine`, `printStr`, and `readLine` are wrappers over the standard handles.
`printStr` flushes after writing so prompts remain visible. See
[`io.md`](io.md) for handle ownership, strict text input, buffering, and errors.

`readLine` returns a line without the trailing newline. At EOF it returns `""`.

`readFile`, `writeFile`, and `appendFile` are whole-file UTF-8 text operations.

`Bytes` is immutable arbitrary byte data, distinct from UTF-8 `String`.
`stringToBytes` encodes UTF-8; `bytesToString` returns `Err (InvalidUtf8 ...)`
instead of replacing invalid input. Byte indices are zero-based, out-of-range
indexing returns `Nothing`, and slicing clamps negative and oversized bounds.

`listen` binds an IPv4 loopback listener. Port zero asks the OS to choose a
free port, returned with the listener as `MkPair listener actualPort`.
`resolveHost` resolves a name on the blocking IO pool and returns one IPv4
address, so it does not stall the scheduler or the readiness thread. `connect`
accepts that IP literal or `localhost`; it does not perform DNS itself.
`sendBytes` completes only after the full buffer is sent. `recvBytes` returns
at most its non-negative maximum and returns empty bytes at EOF. Sockets are
closed only by explicit, idempotent `closeSocket`; GC does not close them.
Relative paths are resolved against the Solveig project root. Their host
failures are ordinary `IOError` values and can be handled with `catchError`.
`finally` runs cleanup after success, typed failure, a defect, or interruption.
The action runs with the caller's interruption state restored and cleanup runs
masked. `bracket` masks acquisition and cleanup while restoring the prior state
for resource use, closing the cancellation window between acquisition and
cleanup installation. A cleanup failure replaces the saved outcome.

`withThread` scopes a spawned child to its body. When the body succeeds, fails,
defects, or is interrupted, the child is interrupted and its cleanup finishes
before the scope exits. `race` returns the first result and cancels the loser;
`concurrently` returns both results and cancels the remaining action as soon as
one fails. Both propagate the winning typed error or defect without changing
its constructor. `timeout milliseconds action` returns `Nothing` after the
deadline or `Just value` when the action finishes first. These combinators use
private channels in Prelude code; a child never outlives the scope that created
it.

See [concurrency.md](concurrency.md) for the scheduler model, fairness and
determinism guarantees, interruption rules, and concurrency testing strategy.

`killThread` requests asynchronous interruption and returns immediately.
Delivery occurs at the target's next VM safe point; killing an already finished
or unknown thread succeeds. `mask` defers delivery while its action remains
runnable. Operations that actually block on a channel or host IO remain
interruptible, so masked code cannot become permanently unkillable. Nested
masks restore one depth at a time.

`sleep milliseconds` parks only the current lightweight thread. Non-positive
durations complete immediately. Normal execution uses monotonic elapsed time;
deterministic execution advances virtual time when no process is runnable, so
concurrency tests do not wait on or assert against the wall clock. A sleeping
thread remains interruptible, including inside `mask`.

`error` raises `Error`; integer division and modulo by zero raise
`DivisionByZero`. The VM's managed stack and heap-object ceilings raise
`StackExhausted` and `HeapExhausted`. Because Solveig is strict, the protected
action must be constructed before `catchDefect` installs its scope; pure work
that should be protected belongs inside an executed IO callback.

Channels are typed FIFO queues shared by lightweight threads. Unbounded sends
enqueue immediately; bounded sends block when the queue is full. Receives block
only while an open queue is empty, and `tryRecv` returns `Nothing` in that
state. Closing preserves queued values, then raises `ChannelClosed` from
further receives and sends. Waiting senders and receivers are resumed FIFO.

Example:

```solveig
main : Task ()
main = do
    printStr "name> "
    name <- readLine
    printLine (appendString "hello, " name)
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
subtract : forall a. Num a => a -> a -> a

class Num a => Fractional a where
    (/) : a -> a -> a
```

`subtract x y` is `y - x`, matching Haskell's section-friendly helper.
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

Instances are provided for `Maybe`, `List`, and `IO e` where appropriate.
Solveig's `do` notation desugars through this generic `Monad` class. A bind
statement `x <- action` uses `bind`, an ordinary expression statement before
the final line uses `thenM`, and the final statement is returned directly.

Example:

```solveig
readTwo : Task [String]
readTwo = mapM
    (fun prompt => do
        printStr prompt
        readLine)
    ["first> ", "second> "]
```

### IO Helpers

```solveig
getArgs : forall e. IO e [String]
seqIO : forall e a b. IO e a -> IO e b -> IO e b
mapIO_ : forall e a. (a -> IO e ()) -> [a] -> IO e ()
```

`seqIO left right` runs `left`, discards its result, then runs `right`.

`mapIO_ f xs` runs an IO action for each list element and discards all results.

Example:

```solveig
main : Task ()
main = do
    args <- getArgs
    mapIO_ printLine args
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

pureIO : forall e a. a -> IO e a
bindIO : forall e a b. IO e a -> (a -> IO e b) -> IO e b
failIO : forall e a. e -> IO e a
error : forall a. String -> a
catchIO : forall e f a. IO e a -> (e -> IO f a) -> IO f a
catchDefectIO : forall e a. IO e a -> (Defect -> IO e a) -> IO e a
raiseDefectIO : forall e a. Defect -> IO e a
finallyIO : forall e a b. IO e a -> IO e b -> IO e a
printLine : String -> IO IOError ()
printStr : String -> IO IOError ()
readLine : IO IOError String
getArgs : forall e. IO e [String]
readFile : String -> IO IOError String
writeFile : String -> String -> IO IOError ()
appendFile : String -> String -> IO IOError ()
openFile : String -> IOMode -> IO IOError Handle
hClose : Handle -> IO IOError ()
hFlush : Handle -> IO IOError ()
hGetLine : Handle -> IO IOError String
hGetContents : Handle -> IO IOError String
hPutStr : Handle -> String -> IO IOError ()
hPutStrLn : Handle -> String -> IO IOError ()
hIsEOF : Handle -> IO IOError Bool
hSetBuffering : Handle -> BufferMode -> IO IOError ()
stdin : Handle
stdout : Handle
stderr : Handle
spawn : forall childError e. IO childError () -> IO e ThreadId
killThread : ThreadId -> IO IOError ()
sleep : Int -> IO IOError ()
maskIO : forall e a. IO e a -> IO e a
restoreMaskIO : forall e a. IO e a -> IO e a
newChan : forall e a. IO e (Chan a)
newBoundedChan : forall e a. Int -> IO e (Chan a)
send : forall a. Chan a -> a -> IO IOError ()
recv : forall a. Chan a -> IO IOError a
tryRecv : forall a. Chan a -> IO IOError (Maybe a)
closeChan : forall a. Chan a -> IO IOError ()
```

`raiseDefectIO` is a low-level operation reserved for Prelude composition. User
code should normally raise `Error` with `error` or let the originating defect
propagate without catching it.

Primitive Int add/sub/mul/neg wrap on `i64` overflow. `intDiv` and `intMod`
raise `DivisionByZero` defects for a zero divisor. String indexing and slicing
count Unicode scalar values.
