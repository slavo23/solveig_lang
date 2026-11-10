# 5. Organize and test an application

Log Audit fits in one source file, but Release Radar has distinct policies for
HTTP, JSON/domain decoding, caching, a local fixture, and orchestration. This
chapter explains the tools that let those parts remain small: polymorphism,
higher-order functions, modules, aliases, typeclasses, and persistent
collections.

**Skip ahead:** experienced FP readers should inspect
[`examples/release_radar/src`](../../examples/release_radar/src/) and read
[modules as architecture](#modules-as-architecture) plus
[testing the boundaries](#testing-the-boundaries).

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

## Modules as architecture

Every `.sv` file below `src/` is a module. For package `shop`,
`src/order/price.sv` has the fully qualified name `shop.order.price`.

Inside the same package, import it relatively:

```solveig
import order.price(total)
```

Select names to keep the local namespace small, or import the module and use a
qualified reference:

```solveig
import std.list(*)

firstThree : forall a. [a] -> [a]
firstThree = std.list.take 3
```

When two unqualified imports provide the same name, Solveig reports ambiguity
instead of silently selecting one.

Release Radar uses the same mechanism for an application:

```text
src/
├── main.sv
└── release_radar/
    ├── cache.sv
    ├── client.sv
    ├── fixture.sv
    └── model.sv
```

`main.sv` imports responsibilities, not one undifferentiated utility module:

```solveig
import release_radar.cache(*)
import release_radar.client(*)
import release_radar.fixture(*)
import release_radar.model(*)
```

- `model` owns domain types, JSON decoding, status interpretation, and
  rendering.
- `client` owns request construction, transport errors, and fetching.
- `cache` owns the persistent ETag format and atomic replacement.
- `fixture` owns the loopback HTTP service used by the hermetic mode.
- `main` owns configuration, mode selection, measurement, and reporting.

This direction keeps transport code dependent on the domain model, while the
model does not depend on the application's orchestration.

For a reusable package, declare its external surface:

```solveig
export (Config, loadConfig)
private (validateRawConfig)
```

`Config(..)` exports public constructors; `Config` alone keeps them opaque
outside the package. A `reexport package.module(...)` declaration can build a
facade. Release Radar is an application and uses package-internal modules, so
it does not need to manufacture a public library API.

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
- `Functor`, `Applicative`, and `Monad` for structured computation;
- `Foldable` and `Traversable` for finite container-wide operations;
- `Hashable` for equality-respecting hashes.

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

## Foldable, Traversable, and Hashable

`foldl` is a strict, stack-safe left fold over any `Foldable` container. Unlike
lazy Haskell, strict Solveig does not need a second `foldl'` name:

```solveig
sum : [Int] -> Int
sum = foldl (+) 0
```

`traverse` applies an `Applicative` effect from left to right while retaining
the container's shape:

```solveig
positive : Int -> Either String Int
positive value =
    if value > 0 then Right value else Left "expected a positive number"

validateAll : [Int] -> Either String [Int]
validateAll = traverse positive
```

For `IO`, the actions run sequentially in input order. Use the explicit
structured-concurrency operations from `std.task` when work should overlap.
`sequenceA` is `traverse id` for a container that already holds effects.

`hash value` is available for `Hashable` values. Equal values are guaranteed to
hash equally, but the exact integer is not a storage or wire format. `Float`
has no `Hashable` instance because NaN makes ordinary floating equality
non-reflexive.

## Choose a collection by its job

The standard library offers persistent collections: an update returns a new
value while the old value remains valid. That fits pure functions naturally and
makes sharing values between threads safe.

Use the collection whose operations describe the problem:

| Need | Type | Typical operations |
| --- | --- | --- |
| Recursive traversal, prepend, small pipelines | `[a]` | `map`, `filter`, `foldl`, `(::)` |
| Indexed finite sequence | `std.vector.Vector a` | `fromList`, `index`, `update`, `length` |
| Deterministic key order | `std.map.Map k v` | `lookup`, `insert`, `delete`, `toAscList` |
| Equality-key lookup when sorted output is irrelevant | `std.hashmap.HashMap k v` | `lookup`, `insert`, `member` |
| Unique values with deterministic order | `std.set.Set a` | `member`, `insert`, `union` |
| First-in, first-out pure work queue | `std.queue.Queue a` | `enqueue`, `dequeue` |

For example, accumulating request counts by route is a map problem rather than
a repeated list-search problem:

```solveig
import std.map

increment : String -> std.map.Map String Int -> std.map.Map String Int
increment route counts =
    let current =
        case std.map.lookup route counts of
            Nothing => 0
            | Just count => count
    in std.map.insert route (current + 1) counts

routeCounts : [String] -> std.map.Map String Int
routeCounts = foldl (flip increment) std.map.empty
```

Iteration order is an API decision. `Map` and `Set` give deterministic ordered
traversal. A `HashMap` is appropriate when equality lookup dominates and output
order is irrelevant; do not serialize its traversal order as a stable format.
A `Vector` has safe lookup returning `Maybe`; absence at an external index is
data, not a reason to introduce a partial helper.

A persistent collection is still strict and finite. It does not make a large
file or HTTP response incremental. Use a scoped `Stream` for input that should
be pulled only as the consumer asks for it, then fold into the smallest strict
summary the application needs.

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

## Testing the boundaries

Solveig currently has no special test declaration or hidden test runtime. A
test is an ordinary package or entry point that computes checks, prints useful
context, and exits nonzero on failure. Repository integration tests also
compile and execute examples through the same driver used by the CLI.

Good application boundaries make deterministic checks straightforward:

- `analyzeLog`, `includeResult`, `decodeReleaseJson`, and cache merging are
  pure functions.
- Release Radar's default mode starts its own loopback TCP listener and calls
  the real HTTP client against it.
- The live GitHub mode is opt-in; it is not required for the hermetic course or
  repository validation.
- Seeded `StdGen` values make generated cases replayable.
- `SOLVEIG_SCHED_DETERMINISTIC=1` is a test/debug scheduler mode, not a
  semantic guarantee that production concurrency has one completion order.

Test observable contracts rather than private call structure. Log Audit's
golden check asserts the exact report. Release Radar's local path asserts typed
decoding and that independent delayed requests overlap, while final results are
restored to input order.

## Deliberate error: importing the wrong layer

If `release_radar.model` imports `release_radar.client` while `client`
already imports `model`, the package has a module cycle. The compiler rejects
that architecture; it does not initialize modules in a guessed order. Move the
shared domain type to the lower-level module or invert the dependency.

Likewise, two wildcard imports that expose the same unqualified name produce an
ambiguity diagnostic. Repair the namespace with a precise import or a qualified
reference, not by relying on import order.

## Checkpoint

Run both applications:

```sh
cargo run -- --project-root examples/log_audit
cargo run -- --project-root examples/release_radar
```

Then trace one Release Radar result through `client.fetchRelease`,
`model.decodeResponse`, and `main.printSummary`. Each transition should
cross a named module boundary for a reason.

## Exercises

1. Decide whether Log Audit's `LogCounts` needs a `Show` instance or whether
   its user-facing `renderReport` is the better abstraction.
2. Replace a list lookup problem with either `Map` or `HashMap`; justify the
   iteration-order contract.
3. Sketch a library facade for the Release Radar decoder using `export` and
   `reexport`, without exposing constructors that callers should not build.
4. Identify one place where point-free style improves the actual source and
   one where it would hide error policy.

## Recap

- Polymorphism restricts implementations to behavior valid for every type.
- Higher-order functions and partial application let applications name policy
  once and supply context gradually.
- `data` creates a distinct type; `type` only gives an existing type a name.
- Modules should follow responsibilities and dependency direction.
- Typeclasses express shared operations; instances supply type-specific
  behavior.
- Persistent collections preserve old values, but remain strict and finite.
- Tests are ordinary, deterministic programs plus repository integration
  checks; external live services are not a prerequisite.

Next: [Decode an external service response](08-parsers-and-json.md).
