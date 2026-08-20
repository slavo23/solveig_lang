# The Solveig Tutorial

Solveig is a strict, statically typed functional language with algebraic data
types, higher-rank functions, typeclasses, typed IO errors, lightweight green
threads, typed channels, structured concurrency, and a bytecode VM.

This tutorial builds a mental model before introducing APIs. The early sections
cover pure programs; the second half develops practical IO and concurrency
patterns through several independent examples. You can read it front to back,
or jump to [IO as a value](#11-io-as-a-value) if you already know functional
programming.

All commands assume the repository root and the `solveig` executable. When
working from this checkout, replace `solveig` with `cargo run --`:

```sh
cargo run -- --project-root examples/quiz_game
```

## 1. Your first project

A project contains a strict `solveig.toml` manifest and `.sv` source files
under `src/`. The manifest selects the entry module.

```text
hello/
|-- solveig.toml
`-- src/
    `-- main.sv
```

Create `hello/solveig.toml`:

```toml
[package]
name = "hello"
version = "0.1.0"

[language]
entry = "main"
```

Create `hello/src/main.sv`:

```solveig
main : String
main = "hello, Solveig"
```

Run it:

```sh
solveig --project-root hello
```

This `main` is pure. Solveig evaluates it and displays the result through its
`Show` instance. Later, executable programs will conventionally use
`main : Task ()` and perform their own output.

Three habits are worth adopting immediately:

1. Give top-level definitions signatures. They are documentation and a stable
   boundary for the checker.
2. Keep calculation pure, then put IO around it. Pure functions are easy to
   reuse, test, and run concurrently.
3. Model alternatives with data constructors instead of sentinel values.

## 2. Values, functions, and expressions

The compiler-known primitive types are:

```solveig
Int
Float
Char
String
Bool
()
```

`Int` is signed 64-bit, `Float` is IEEE-754 `f64`, `String` is immutable UTF-8
text, and `()` is the unit value.

Functions are applied by whitespace:

```solveig
subtotal : Int -> Int -> Int
subtotal price quantity = price * quantity

main : Int
main = subtotal 125 4
```

Application binds more tightly than infix operators, so `subtotal 125 4 + 10`
means `(subtotal 125 4) + 10`.

Functions are curried. Applying fewer arguments returns a function:

```solveig
add : Int -> Int -> Int
add left right = left + right

addTax : Int -> Int
addTax = add 20
```

Anonymous functions use `fun`:

```solveig
triple : Int -> Int
triple = fun value => value * 3
```

Everything below is an expression and therefore produces a value:

```solveig
shippingCost : Int -> Int
shippingCost basketTotal =
    let threshold = 5000 in
        if basketTotal >= threshold then 0 else 499
```

Solveig is strict: function arguments and local bindings are evaluated before
the function body needs them. This matters for performance, defects, and IO
error boundaries later.

Use `appendString`, not `+`, for text:

```solveig
greeting : String -> String
greeting name = appendString "Welcome, " name
```

Numeric operators come from `Num`, and there is intentionally no
`Num String` instance.

## 3. Model a domain with algebraic data types

Suppose an order can be waiting, paid, or rejected with an explanation:

```solveig
data OrderStatus =
    Waiting
  | Paid Int
  | Rejected String
```

Constructors are ordinary typed functions:

```solveig
Waiting : OrderStatus
Paid : Int -> OrderStatus
Rejected : String -> OrderStatus
```

Inspect a value with `case`:

```solveig
statusLabel : OrderStatus -> String
statusLabel status =
    case status of
        Waiting => "waiting for payment"
        | Paid cents => appendString "paid " (show cents)
        | Rejected reason => appendString "rejected: " reason
```

The checker understands constructor coverage and reports a non-exhaustive
match. A wildcard is useful only when the unobserved cases genuinely share
semantics:

```solveig
isPaid : OrderStatus -> Bool
isPaid status =
    case status of
        Paid amount => True
        | _ => False
```

Product types hold several values together:

```solveig
data Customer = Customer String String

customerEmail : Customer -> String
customerEmail (Customer name email) = email
```

Constructors are positional today; there is no record syntax. Small accessor
functions give callers a named interface when positional fields become hard to
remember.

## 4. Optional values, expected alternatives, and defects

Use the shape that matches the domain:

```solveig
Maybe a       -- Just value | Nothing
Either a b    -- Left value | Right value
Result e a    -- Ok value | Err error
```

`Maybe` is appropriate when absence is expected and needs no explanation:

```solveig
findFirstPaid : [OrderStatus] -> Maybe Int
findFirstPaid statuses =
    case statuses of
        [] => Nothing
        | (Paid cents) :: rest => Just cents
        | other :: rest => findFirstPaid rest
```

`Result` is a pure success-or-failure value:

```solveig
validateQuantity : Int -> Result String Int
validateQuantity quantity =
    if quantity > 0 then
        Ok quantity
    else
        Err "quantity must be positive"
```

These are data, not effects. Constructing `Err` does not interrupt evaluation.
By contrast, `error "message"` raises a defect and unwinds execution. Reserve
defects for broken assumptions or deliberately unrecoverable code. Expected
business outcomes belong in `Maybe`, `Result`, or a domain-specific data type.

## 5. Lists and recursive processing

List literals and patterns are conventional:

```solveig
prices : [Int]
prices = [125, 250, 499]

total : [Int] -> Int
total values =
    case values of
        [] => 0
        | value :: rest => value + total rest
```

The Prelude includes the ubiquitous traversal operations `map`, `filter`,
`foldl`, `foldr`, `append`, and `concat`. Extended operations are explicit:

```solveig
import std.list(take, drop, takeWhile, dropWhile, zip, zipWith, reverse, range)
```

A practical calculation is often a short pipeline:

```solveig
expensiveTotal : [Int] -> Int
expensiveTotal prices =
    foldl
        (fun running price => running + price)
        0
        (filter (fun price => price >= 1000) prices)
```

Use `foldl` with a strict accumulator for summaries. Use `foldr` when the
result naturally follows the list's constructor shape, such as rebuilding a
list. Recursive definitions remain valuable when the state transition needs a
name or several accumulators.

The current `List` is linked, so repeated indexing is not a good array
substitute. Prefer one traversal, a fold, or a domain structure designed for
the access pattern.

## 6. Multiple equations and pattern-directed functions

Top-level functions and explicit instance methods can use several equations:

```solveig
sum : [Int] -> Int
sum [] = 0
sum (value :: rest) = value + sum rest

describeStatus : OrderStatus -> String
describeStatus Waiting = "waiting"
describeStatus (Paid cents) = appendString "paid " (show cents)
describeStatus (Rejected reason) = appendString "rejected: " reason
```

Equations are tried from top to bottom. Keep specific patterns before broad
ones. All equations for one function or instance method must be contiguous and
agree on arity.

## 7. Modules and imports

Paths determine module names:

```text
shop/
`-- src/
    |-- main.sv             -> main
    `-- orders/
        `-- pricing.sv      -> orders.pricing
```

`src/orders/pricing.sv`:

```solveig
discount : Int -> Int
discount cents = if cents >= 10000 then 1000 else 0

finalPrice : Int -> Int
finalPrice cents = cents - discount cents
```

`src/main.sv` can import selected definitions:

```solveig
import orders.pricing(finalPrice)

main : Int
main = finalPrice 12500
```

Other forms are:

```solveig
import orders.pricing
import orders.pricing as pricing
```

Every normal module implicitly imports the focused `prelude.builtins`,
`prelude.list`, `prelude.typeclasses`, `prelude.function`, `prelude.control`,
`prelude.show`, and `prelude.instances` modules. Domain APIs live in the
non-implicit `std` package:

```solveig
import std.io(readFile)
import std.list(take, zip)
import std.text(stringToChars)
```

Low-level compiler primitives remain in `prelude.prim`. Application code should
prefer the public `std.*` wrapper where one exists. The explicit split keeps a
new file, network, or collection operation from entering every module's scope.

## 8. Polymorphism, aliases, and higher-rank functions

Explicit polymorphism uses `forall`:

```solveig
identity : forall a. a -> a
identity value = value

chooseFirst : forall a b. a -> b -> a
chooseFirst first second = first
```

Type aliases name an existing type without creating a runtime wrapper:

```solveig
type UserId = Int
type Named a = Pair String a
type UserScores = Named [Int]
```

Aliases must be fully applied. Use a `data` declaration instead when values
need a genuinely distinct type or constructor-level validation.

Solveig also supports higher-rank arguments. Parentheses make the inner
quantifier's scope explicit:

```solveig
useAtTwoTypes : (forall a. a -> a) -> Pair Int Bool
useAtTwoTypes transform = MkPair (transform 7) (transform True)
```

The caller must provide one function that works for every `a`; it cannot pass
a function specialized only to `Int`.

## 9. Typeclasses

Typeclasses describe operations shared by several types:

```solveig
class Render a where
    render : a -> String

data Priority = Low | High

instance Render Priority where
    render Low = "low"
    render High = "high"
```

An instance method's equations are checked together, so Solveig reports
non-exhaustive or unreachable patterns just as it does for a top-level
function. The method's type remains declared once in the class; instance-local
method signatures are not part of the language yet. Class default bodies are
still single definitions.

Constraints appear before `=>`:

```solveig
renderPair : forall a b. (Render a, Render b) => Pair a b -> String
renderPair (MkPair left right) =
    appendString (render left) (appendString " / " (render right))
```

The standard classes include `Eq`, `PartialOrd`, `Ord`, `Num`, `Fractional`, `Show`,
`Functor`, `Applicative`, `Monad`, `Semigroup`, and `Monoid`. Typeclass
resolution is static; there is no runtime reflection or method lookup by text.

`PartialOrd` is intentionally separate from `Ord`. In particular, Float is
partially ordered because NaN is unordered:

```solveig
partialCompare (0.0 / 0.0) 1.0  -- Nothing
totalCompareFloat (0.0 / 0.0) 1.0  -- explicit deterministic IEEE ordering
```

`Show` controls ordinary value rendering:

```solveig
instance Show Priority where
    show Low = "Low"
    show High = "High"
```

Do not make every domain type printable merely for debugging. A deliberate
`renderInvoice` or `toCsvRow` often communicates a stronger output contract
than a general `Show` instance.

## 10. Operators and fixity

Symbolic functions may declare precedence and associativity:

```solveig
infixl 6 <+>

(<+>) : Int -> Int -> Int
(<+>) left right = left + right + 1
```

Named functions can be used infix with backticks:

```solveig
infixr 8 `thenCode`

thenCode : Int -> Int -> Int
thenCode left right = left * 100 + right

main = 1 `thenCode` 2 `thenCode` 3
```

Sections create functions:

```solveig
addOne = (+ 1)
subtractFromTen = (10 -)
```

Following Haskell, `(- 1)` is unary negation, not a subtraction section. Use
`(subtract 1)` for a function that subtracts one from its argument. Operators
without declarations default to `infixl 9`; parenthesize mixed expressions
when the intended grouping is not obvious.

## 11. IO as a value

The practical IO sections use explicit domain imports. A module only needs the
ones matching the work it performs:

```solveig
import std.env
import std.io
import std.fs
import std.process
import std.channel
import std.thread
import std.task
import std.time
import std.random
import std.bytes
import std.net
```

This distinction is the foundation of practical Solveig:

```solveig
String             -- text that already exists
Task String        -- an action that may perform IO and eventually produce text
```

`Task a` is an alias for `IO IOError a`. `IO` has two parameters:

```solveig
IO errorType successType
```

An IO action is inert data until the runtime performs it from an IO `main`.
Calling `readFile "orders.txt"` constructs an action; it does not secretly read
during pure evaluation. This separation lets the type checker show where
effects occur and lets the runtime suspend only the green thread performing a
blocking operation.

The primitive building blocks are:

```solveig
pureIO : forall e a. a -> IO e a
bindIO : forall e a b. IO e a -> (a -> IO e b) -> IO e b
failIO : forall e a. e -> IO e a
```

- `pureIO value` succeeds without host work.
- `bindIO action continuation` performs `action`, then passes its success to
  `continuation`.
- `failIO error` produces a typed failure in the action's error channel.

`do` notation is readable syntax for those operations:

```solveig
main : Task ()
main = do
    printStr "customer name> "
    name <- readLine
    let message = appendString "creating account for " name
    printLine message
```

Within a `do` block:

- `name <- action` binds an action's success value;
- `let name = expression` creates a pure local value;
- an ordinary action line is sequenced and its result discarded;
- the last line is the result of the block.

Pattern binds after `<-` are not supported yet. Bind first, then use `case`:

```solveig
readTwoClocks : Task ()
readTwoClocks = do
    pair <- concurrently getMonotonicTime getSystemTime
    case pair of
        MkPair monotonic system => printLine (show monotonic)
```

Use `mapM` when every input produces an output, and `mapIO_` when only
sequencing matters:

```solveig
readDocuments : [String] -> Task [String]
readDocuments paths = mapM readFile paths

printDocuments : [String] -> Task ()
printDocuments documents = mapIO_ printLine documents
```

Both traverse sequentially. A list of IO actions is not automatically
concurrent; explicit `concurrently`, scoped workers, or channels make that
policy visible.

Do notation is not IO-specific. It desugars through `Monad`, so it also works
with `Maybe`, `List`, and user-defined monads.

## 12. A practical command-line workflow

This example is a tiny shift-note tool. It demonstrates arguments, a prompt,
whole-file output, and a small pure rendering core. It is intentionally
different from the concurrent log-audit capstone.

```text
shift-note/
`-- src/
    `-- main.sv
```

```solveig
import std.env(getArgs)
import std.io(writeFile)

notePath : [String] -> String
notePath args =
    case args of
        path :: rest => path
        | [] => "shift-note.txt"

renderNote : String -> String -> String
renderNote operator note =
    appendString
        "operator: "
        (appendString operator (appendString "\nnote: " (appendString note "\n")))

main : Task ()
main = do
    args <- getArgs
    printStr "operator> "
    operator <- readLine
    printStr "handover note> "
    note <- readLine
    let path = notePath args
    writeFile path (renderNote operator note)
    printLine (appendString "saved " path)
```

Run with the default path:

```sh
solveig --project-root shift-note
```

Or pass a path after `--`, which separates Solveig CLI options from program
arguments:

```sh
solveig --project-root shift-note -- night.txt
```

Relative paths start at the project root's logical working directory. The
runtime owns that working directory; changing it with `setCurrentDirectory`
does not mutate the host process's global directory.

Keep `renderNote` pure. You can test it without files or input streams, and a
future JSON or database frontend could reuse the same validated content.

## 13. Whole files, handles, and resource ownership

The functions in this section come from `std.io`:

```solveig
import std.io
```

Use whole-file functions when the complete value comfortably fits memory:

```solveig
readFile : String -> Task String
writeFile : String -> String -> Task ()
appendFile : String -> String -> Task ()
readFileBytes : String -> Task Bytes
writeFileBytes : String -> Bytes -> Task ()
```

Text operations require UTF-8. Byte operations preserve arbitrary bytes.

Use handles for incremental IO, explicit buffering, or a resource that must
remain open across several operations:

```solveig
openFile : String -> IOMode -> Task Handle
hGetLine : Handle -> Task String
hGetContents : Handle -> Task String
hPutStr : Handle -> String -> Task ()
hPutStrLn : Handle -> String -> Task ()
hFlush : Handle -> Task ()
hClose : Handle -> Task ()
```

The garbage collector does not close handles. Use `withFile`, which installs
cleanup before giving your code the handle:

```solveig
copyWithHeader : String -> String -> Task ()
copyWithHeader source destination =
    withFile source ReadMode (fun input => do
        contents <- hGetContents input
        withFile destination WriteMode (fun output => do
            hPutStrLn output "# generated copy"
            hPutStr output contents))
```

Both handles close after success, typed failure, defect, or interruption.
`hGetContents` is strict: it reads and decodes everything before succeeding.
For unbounded input, loop over `hGetLine`/`hIsEOF` or use bounded byte chunks
with `hGetBytes`.

Buffering controls when successful writes reach the host:

```solveig
hSetBuffering output (BlockBuffering (Just 16384))
```

`NoBuffering` writes every operation, `LineBuffering` flushes after a newline,
and `BlockBuffering` batches bytes. `printStr` explicitly flushes, which is why
interactive prompts remain visible even when stdout is redirected.

The ownership rule extends beyond files: sockets also require explicit
`closeSocket`. Prefer `bracket acquire release use` whenever a host resource
has a clear lifetime.

## 14. Typed IO errors, defects, and interruption

Solveig has three different failure mechanisms because they answer different
questions.

### Typed errors: an operation can normally fail

`Task a` exposes `IOError` in its type. `readFile` may produce a
`ReadFileError path message`; a channel may produce `ChannelClosed`; a socket
connect may produce `ConnectError message`.

Handle an expected failure with `catchError`:

```solveig
readConfiguration : Task String
readConfiguration =
    catchError
        (readFile "config/production.txt")
        (fun error =>
            case error of
                ReadFileError path message => do
                    printLine (appendString "using defaults; could not read " path)
                    pureIO "workers=2\n"
                | other => failIO other)
```

The handler returns the same success type, here `String`. It may recover,
translate the error with `mapError`, or rethrow with `failIO`.

Use `orElse primary fallback` when the error details do not matter and the
fallback should run for any typed failure. Use explicit `catchError` when the
constructor affects policy or should be logged.

### Defects: the program violated an assumption

`error`, division by zero, managed stack exhaustion, and managed heap
exhaustion produce `Defect` values. `catchError` cannot catch them.

`catchDefect` is useful at a deliberate isolation boundary, such as keeping a
long-running job host alive after one plugin violates an invariant. It should
not replace typed errors in ordinary APIs.

Because Solveig is strict, pure work passed as an argument runs while the
protected action is being constructed. Put defect-prone work inside an action
callback if the scope must catch it:

```solveig
divideInsideScope : Task ()
divideInsideScope =
    catchDefect
        (bindIO
            (pureIO ())
            (fun ready => printLine (show (div 10 0))))
        (fun defect => printLine "calculation failed")
```

### Interruption: another thread cancels this work

Interruption is asynchronous control flow delivered at VM safe points or while
a thread is blocked. Neither typed-error nor defect handlers consume it.
Installed `finally` and `bracket` cleanup still runs.

This distinction prevents a broad error handler from accidentally ignoring
cancellation and leaving a structured scope unable to finish.

## 15. Cleanup combinators

Cleanup and defect scopes are provided by `std.task`:

```solveig
import std.task(catchDefect, finally, bracket)
```

Use cleanup policy instead of open-coded acquire/use/release sequences:

```solveig
finally : IO e a -> IO e b -> IO e a

bracket :
    IO e resource ->
    (resource -> IO e cleanup) ->
    (resource -> IO e a) ->
    IO e a
```

`finally action cleanup` preserves `action`'s outcome when cleanup succeeds.
`bracket acquire release use` also closes the cancellation window between
acquisition and cleanup installation.

Cleanup runs after success, typed failure, a defect, or interruption. A
cleanup failure replaces the earlier outcome because Solveig cannot honestly
claim that a required release succeeded.

Cleanup runs masked against interruption while runnable, but a blocking
cleanup remains interruptible. There is no uninterruptible mask that could
make a thread permanently unkillable.

## 16. The concurrency model

Solveig concurrency is blocking-style code over lightweight green threads:

- one OS thread evaluates Solveig bytecode;
- each green thread has its own stacks and instruction state;
- all green threads share the VM heap;
- blocking file, terminal, DNS, and process work uses a bounded host worker
  pool;
- TCP readiness uses one event-loop thread;
- host threads never evaluate Solveig code or inspect VM values.

This is concurrency, not parallel execution of pure Solveig calculations. Its
purpose is to keep many independent tasks making progress while some wait on
IO, channels, or timers.

The low-level primitive is:

```solveig
spawn : IO childError () -> IO e ThreadId
```

Raw `spawn` is intentionally unstructured. Main may finish while raw children
remain, and a child's failure does not automatically become its parent's typed
error. Prefer `withThread`, `concurrently`, `race`, or `timeout` when lifetime
or results matter.

## 17. Typed channels and backpressure

Channel operations are explicit:

```solveig
import std.channel
import std.thread(spawn)
import std.task(withThread, concurrently)
import std.time(sleep)
```

Channels are mutable FIFO queues with a type for their messages:

```solveig
newChan : IO e (Chan a)
newBoundedChan : Int -> IO e (Chan a)
send : Chan a -> a -> Task ()
recv : Chan a -> Task a
tryRecv : Chan a -> Task (Maybe a)
closeChan : Chan a -> Task ()
```

An unbounded channel lets producers allocate queued work without a fixed
limit. A bounded channel parks a sender when full. That is backpressure: the
consumer's pace limits queued memory and upstream work.

Closing is explicit and idempotent. Queued values remain readable; after they
are drained, `recv` fails with `ChannelClosed`. A worker loop can treat that
typed error as its normal stopping signal.

### Real-world example: an order-fulfilment pipeline

The following core uses two workers to validate queued orders. It is not the
Phase L file-audit capstone: the work is a domain pipeline, and its simulated
delay stands in for a remote warehouse call.

```solveig
data Order = Order Int Int

data Fulfilment =
    Accepted Int
  | RejectedOrder Int String

validateOrder : Order -> Fulfilment
validateOrder (Order orderId quantity) =
    if quantity <= 0 then
        RejectedOrder orderId "quantity must be positive"
    else if quantity > 50 then
        RejectedOrder orderId "manual review required"
    else
        Accepted orderId

processOrder : Chan Fulfilment -> Order -> Task ()
processOrder results order = do
    sleep 10
    send results (validateOrder order)

worker : Chan Order -> Chan Fulfilment -> Task ()
worker jobs results =
    catchError
        (workerStep jobs results)
        (fun error =>
            case error of
                ChannelClosed => pureIO ()
                | other => failIO other)

workerStep : Chan Order -> Chan Fulfilment -> Task ()
workerStep jobs results = do
    order <- recv jobs
    processOrder results order
    worker jobs results

produce : Chan Order -> Task ()
produce jobs = do
    send jobs (Order 1001 4)
    send jobs (Order 1002 0)
    send jobs (Order 1003 80)
    send jobs (Order 1004 2)
    closeChan jobs

countAccepted : Fulfilment -> Int
countAccepted result =
    case result of
        Accepted orderId => 1
        | RejectedOrder orderId reason => 0

collect : Chan Fulfilment -> Int -> Int -> Task Int
collect results remaining accepted =
    if remaining == 0 then
        pureIO accepted
    else do
        result <- recv results
        collect results (remaining - 1) (accepted + countAccepted result)
```

The orchestration is the important part:

```solveig
runPipeline : Chan Order -> Chan Fulfilment -> Task Int
runPipeline jobs results =
    withThread
        (worker jobs results)
        (fun firstWorker =>
            withThread
                (worker jobs results)
                (fun secondWorker => do
                    completed <- concurrently
                        (produce jobs)
                        (collect results 4 0)
                    case completed of
                        MkPair produced accepted => pureIO accepted))

main : Task ()
main = do
    jobs <- newBoundedChan 2
    results <- newBoundedChan 2
    accepted <- runPipeline jobs results
    closeChan results
    printLine (appendString "accepted orders: " (show accepted))
```

Why this shape works:

1. The two worker lifetimes are scoped by nested `withThread` calls.
2. Jobs and results are bounded, so neither side has unlimited buffering.
3. Producer and collector run together. Producing everything before collecting
   could deadlock: workers might fill the result channel while the producer is
   blocked on the full job channel.
4. The collector expects exactly one result per input. Completion order does
   not affect the final count.
5. Closing `jobs` lets workers finish naturally; leaving the scope is still a
   final cancellation safety net.

This topology generalizes to image conversion, outbound API requests, email
delivery, validation batches, and background indexing.

The exact program above is also checked by the golden fixture at
`tests/golden/run/tutorial_order_pipeline`, including bounded backpressure and
virtual-time worker delays.

## 18. Structured concurrency

```solveig
import std.task(withThread, concurrently, race, timeout)
```

Structured combinators connect a child's lifetime to a lexical scope:

```solveig
withThread : IO e a -> (ThreadId -> IO e b) -> IO e b
concurrently : IO e a -> IO e b -> IO e (Pair a b)
race : IO e a -> IO e b -> IO e (Either a b)
timeout : Int -> IO e a -> IO e (Maybe a)
```

### Run independent work and keep both values

```solveig
loadDashboard : Task (Pair String String)
loadDashboard = concurrently
    (readFile "cache/customers.txt")
    (readFile "cache/orders.txt")
```

If one side fails, `concurrently` cancels the other and propagates the exact
typed error. It returns only after the cancelled child has finished cleanup.

### Use the first result

```solveig
firstReplica : Task (Either String String)
firstReplica = race
    (readFile "replicas/east.txt")
    (readFile "replicas/west.txt")
```

The losing action is cancelled. Do not use `race` when both side effects must
finish.

### Put a deadline around work

```solveig
loadOptionalSnapshot : Task (Maybe String)
loadOptionalSnapshot = timeout 250 (readFile "snapshots/latest.txt")
```

`Nothing` means the timer won; `Just value` means the action completed.
Milliseconds are used for `sleep` and `timeout`.

Cancellation does not make every operating-system call synchronously
cancellable. A cancelled green thread stops waiting, but an already-running
blocking host request may finish before its worker is reused. Structured scope
still guarantees that Solveig cleanup and child lifetime are settled before
the combinator returns.

### Scope a service-style child

```solveig
heartbeat : Chan String -> Task ()
heartbeat events = do
    sleep 1000
    send events "heartbeat"
    heartbeat events

runSession : Chan String -> Task ()
runSession events =
    withThread
        (heartbeat events)
        (fun heartbeatThread => do
            printLine "session started"
            sleep 5000
            printLine "session finished")
```

When the body finishes or fails, `withThread` interrupts the heartbeat and
waits for its cleanup. The child cannot leak into the next session.

## 19. Determinism, clocks, and randomness

```solveig
import std.time
import std.random
```

Concurrent host IO may finish in any order. Make observable results
deterministic by design:

- aggregate with commutative counts or keyed data;
- attach sequence numbers and reorder at the collector;
- print from one collector instead of every worker;
- synchronize with channels, not elapsed-time guesses;
- expect a known number of results or use an explicit completion message.

The scheduler itself is FIFO for fixed internal inputs, but that does not make
disk, DNS, process, or socket completion order deterministic.

Clock APIs return nanoseconds:

```solveig
getMonotonicTime : Task Int
getSystemTime : Task Int
```

Use monotonic differences for durations:

```solveig
measureRead : String -> Task Int
measureRead path = do
    before <- getMonotonicTime
    contents <- readFile path
    after <- getMonotonicTime
    pureIO (after - before)
```

System time is a signed Unix-epoch value and may move when the host clock is
adjusted. It is not a duration clock.

Pseudorandom state is explicit:

```solveig
mkStdGen : Int -> StdGen
newStdGen : Task StdGen
randomRange : Int -> Int -> StdGen -> Pair Int StdGen
```

Thread the returned generator:

```solveig
rollTwoDice : StdGen -> Pair (Pair Int Int) StdGen
rollTwoDice generator =
    case randomRange 1 6 generator of
        MkPair first afterFirst =>
            case randomRange 1 6 afterFirst of
                MkPair second afterSecond =>
                    MkPair (MkPair first second) afterSecond
```

`mkStdGen` makes tests reproducible. `newStdGen` is the only fresh seeding IO.
The generator is clock-seeded and non-cryptographic: never use it for keys,
tokens, passwords, or security nonces.

## 20. Environment, processes, and TCP

```solveig
import std.env
import std.fs
import std.process
import std.bytes
import std.net
```

Environment and filesystem state are runtime-local where host APIs permit:

```solveig
getEnv : String -> Task (Maybe String)
setEnv : String -> String -> Task ()
getEnvironment : Task [Pair String String]
getCurrentDirectory : Task String
setCurrentDirectory : String -> Task ()
listDirectory : String -> Task [String]
```

`getEnvironment` and `listDirectory` are sorted for deterministic iteration.
Use existence checks for ordinary optional paths; other failures retain their
operation-specific `IOError` constructor.

`runProcess` uses an argument list directly, without a shell:

```solveig
runFormatter : String -> Task ProcessResult
runFormatter path = runProcess "formatter" ["--check", path]

reportProcess : ProcessResult -> Task ()
reportProcess result =
    if processExitCode result == 0 then
        printLine (processStdout result)
    else
        printLine (processStderr result)
```

Non-zero status is data, not an `IOError`. Failure to start or capture the
child is a `RunProcessError`. Output is strictly captured UTF-8, so this API is
not suitable for unbounded or binary subprocess streams.

Program exit is also an IO action:

```solveig
exitWith : forall a. Int -> Task a
exitSuccess : forall a. Task a
exitFailure : forall a. Task a

loadAndReport : Task ()
loadAndReport = do
    configuration <- readFile "config/production.txt"
    printLine "configuration loaded"

runOrExit : Task ()
runOrExit =
    catchError
        loadAndReport
        (fun error => do
            printLine "configuration failed"
            exitFailure)
```

An exit escaping `main` runs installed cleanup, interrupts and drains sibling
green threads, flushes handles, and then returns the status to the host. Typed
error and defect handlers do not suppress it. Use statuses from 0 through 255
for portable command-line behavior.

The TCP surface is byte-oriented:

```solveig
resolveHost : String -> Task String
connect : String -> Int -> Task Socket
listen : Int -> Task (Pair Socket Int)
accept : Socket -> Task Socket
sendBytes : Socket -> Bytes -> Task ()
recvBytes : Socket -> Int -> Task Bytes
closeSocket : Socket -> Task ()
```

Use `bracket` or `finally` to close sockets. The current API is TCP with basic
IPv4 resolution, not a production HTTP client: it has no TLS, redirect,
chunked-transfer, or general IPv6 layer. The concurrent downloader example is
an intentionally constrained demonstration of the machinery.

## 21. Testing IO and concurrent programs

Reliable tests assert protocols and conserved values, not lucky timing.

For ordinary projects:

1. Keep transformations pure and unit-testable through a pure `main` or a
   small golden executable.
2. Put deterministic fixtures inside the project root.
3. Overwrite a fixed generated path and ignore it, instead of creating a new
   timestamped file every run.
4. Read generated output back when the test must prove actual filesystem
   contents.
5. Exercise typed failure explicitly—include a missing path or closed channel
   and assert the recovery result.

For concurrency:

1. Give messages a closed data type.
2. Bound queues whose producers can outrun consumers.
3. Use one collector for observable output.
4. Coordinate with channels or exact result counts.
5. Avoid asserting worker completion order when host IO is involved.
6. Use `SOLVEIG_SCHED_DETERMINISTIC=1` for timer and scheduler tests.
7. Run runtime-sensitive programs under `SOLVEIG_GC_STRESS=1`.

Example commands:

```sh
SOLVEIG_SCHED_DETERMINISTIC=1 solveig --project-root my-project
SOLVEIG_GC_STRESS=1 solveig --project-root my-project
```

Deterministic scheduler mode uses virtual time. When every runnable process is
waiting on timers, the logical clock jumps to the next deadline, so timer tests
do not sleep in real time. It does not virtualize the operating system or force
host IO completions into a stable order.

## 22. Practical performance guidelines

Solveig is strict and designed for practical programs, but data and effect
shapes still matter:

- Prefer one list traversal over repeated `map`/`filter` passes when the input
  is large and allocation matters.
- Use tail-recursive accumulator loops for large summaries.
- Choose bounded channels at ownership boundaries; capacity documents the
  maximum queued work and provides backpressure.
- Do not create one green thread per tiny pure calculation. Concurrency helps
  when work waits or needs independent lifetimes.
- Use whole-file IO only when its strict memory use is acceptable.
- Stream large data with handles or bounded byte chunks.
- Keep values crossing worker boundaries compact; do pure parsing after IO
  returns to the VM unless the language API deliberately owns another model.
- Measure monotonic time around the whole operation, not individual scheduling
  steps.

The runtime yields pure loops at safe points, so one recursive computation
cannot indefinitely monopolize the VM. This is a scheduling guarantee, not a
reason to ignore algorithmic complexity.

## 23. Explore complete projects

The repository includes several programs with different lessons:

- `examples/quiz_game` — interactive input, parsing, whole-file IO, reports,
  and a user-defined `State` monad.
- `examples/log_audit` — the Phase L capstone: bounded file workers, typed
  missing-file recovery, order-independent aggregation, and generated output.
- `examples/concurrent_downloader` — DNS, TCP readiness, bounded channels,
  timeouts, explicit socket cleanup, and an offline loopback test mode.
- `examples/scry` — an S-expression rule engine with recursive search and an
  interactive prompt.
- `examples/smoke_suite` — algorithms and data structures used for broad
  compiler/runtime coverage.

Then use the focused references:

- [Packages](packages.md) for manifests, layout, path dependencies, compiler
  libraries, and IDE edit policy.
- [Prelude reference](prelude.md) for exact signatures.
- [Standard library](standard-library.md) for module boundaries, imports, and
  exact host-facing APIs.
- [Input and output](io.md) for encoding, buffering, platform behavior, and
  resource ownership.
- [Errors, defects, and interruption](errors.md) for recovery and cleanup.
- [Concurrency](concurrency.md) for scheduling, cancellation, channels, TCP,
  and deterministic testing.
- [Diagnostics](diagnostics.md) for compiler error codes.
- [VM](vm.md) for runtime representation and scheduler internals.

The central design lesson is simple: model the domain with pure values, make
failure visible in types, keep resources inside cleanup scopes, and give every
concurrent child and queued message an explicit lifetime.
