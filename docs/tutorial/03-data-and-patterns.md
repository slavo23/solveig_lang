# 3. Data, patterns, and errors

Algebraic data types describe the exact shapes a value may have. Pattern
matching then handles those shapes directly.

## Define a data type

```solveig
data Delivery =
    Pending
  | InTransit String
  | Delivered Int
```

`Delivery` is the type. `Pending`, `InTransit`, and `Delivered` are
constructors. Constructors may carry different data.

Use one equation per shape:

```solveig
status : Delivery -> String
status Pending = "pending"
status (InTransit city) = "in transit through " <> city
status (Delivered timestamp) = "delivered at " <> show timestamp
```

Equations belonging to one function must be adjacent. The compiler checks that
matching is exhaustive and reports redundant branches.

## `case` expressions

Use `case` when the scrutinee is computed locally:

```solveig
isFinished : Delivery -> Bool
isFinished delivery =
    case delivery of
        Delivered _ => True
        | Pending => False
        | InTransit _ => False
```

`_` matches without binding a name. A constructor pattern can be nested inside
another pattern, including lists and tuples.

## Product data and constructor lambdas

```solveig
data User = User String String

email : User -> String
email (User _ address) = address

emails : [User] -> [String]
emails = map (fun (User _ address) => address)
```

Constructor patterns work in top-level equations, `case`, lambdas, and
irrefutable `let` bindings. For example, a one-constructor type is always safe
to unpack with `let`:

```solveig
rename : String -> User -> User
rename newName user =
    let User _ address = user
    in User newName address
```

## `Maybe`: absence is data

`Maybe a` is either `Nothing` or `Just a`:

```solveig
withDefault : forall a. a -> Maybe a -> a
withDefault fallback Nothing = fallback
withDefault _ (Just value) = value
```

Use it when absence is an expected answer, such as a missing map key or an
out-of-range vector index. Callers cannot accidentally treat absence as a
normal value because they must match both constructors.

## `Either`: pure failure is data

`Either e a` is `Right a` or `Left e`. It is useful for parsing, validation, and
decoding that perform no IO:

```solveig
data AgeError = NegativeAge Int

validateAge : Int -> Either AgeError Int
validateAge age =
    if age < 0 then Left (NegativeAge age) else Right age

renderAge : Either AgeError Int -> String
renderAge (Right age) = "age: " <> show age
renderAge (Left (NegativeAge age)) = "invalid age: " <> show age
```

## `IO e a`: effects with typed failure

`IO e a` describes an effectful action that can fail with `e` or succeed with
`a`. `Task a` is an alias for low-level host IO:

```solveig
type Task a = IO HostError a
```

Create success and failure with `pureIO` and `failIO`:

```solveig
data LookupError = NotFound String

lookupName : String -> IO LookupError String
lookupName key =
    if key == "primary" then pureIO "Ada" else failIO (NotFound key)
```

Handle a typed error with `catchError`:

```solveig
recover : IO LookupError String -> Task String
recover action =
    catchError
        (mapError (fun (NotFound key) => HostError DoesNotExist "lookupName" key) action)
        (fun _ => pureIO "anonymous")
```

`mapError` changes only the error channel. `fmap` changes only the success
value. This makes error translation explicit at subsystem boundaries.

## Typed errors, defects, and interruption

Solveig distinguishes three mechanisms:

- A typed error is an expected operational failure represented by `IO e a`.
- A defect is an unchecked programming failure, created by `error` and handled
  only when a boundary deliberately uses `catchDefect`.
- Interruption cancels a running task. Structured concurrency uses interruption
  to stop child work when its scope ends.

Use typed errors for things callers can reasonably recover from: a missing
file, a refused connection, invalid input, or an HTTP transport error. Use
`error` for broken invariants, not ordinary control flow.

## Cleanup is independent of success

`finally action cleanup` runs cleanup when `action` succeeds or raises a typed
error. `bracket acquire release use` protects the whole resource lifetime.
Higher-level helpers such as `withFile`, `withConnection`, `withListener`, and
`withThread` are built around that ownership model.

The compiler also rejects non-exhaustive patterns before the program runs, so
a missing data case is not turned into an implicit runtime error.

Next: [Functions, modules, and typeclasses](04-abstraction.md).
