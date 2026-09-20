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

This is more than convenient syntax. The type rules out contradictory states:
there is no `Delivery` that is both pending and delivered, and a delivered
value always carries its timestamp. If a later version adds
`DeliveryFailed String`, every total match that has not handled failure becomes
a compiler diagnostic. That is useful maintenance pressure at precisely the
places where policy must change.

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
`a`. `Task a` is an alias for low-level I/O:

```solveig
type Task a = IO IOError a
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
        (mapError (fun (NotFound key) => IOError DoesNotExist "lookupName" key) action)
        (fun _ => pureIO "anonymous")
```

`mapError` changes only the error channel. `fmap` changes only the success
value. This makes error translation explicit at subsystem boundaries.

Choose an error type at the layer that can make a recovery decision. An HTTP
client module may retain `HttpError`; a user-service module can wrap it beside
domain cases such as `UserNotFound UserId`; `main` can finally render or log the
combined error. Translating everything to `String` near the bottom destroys
the structure needed to decide whether to retry, return 404, or fix invalid
configuration.

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

`finally action cleanup` runs cleanup after success, typed failure, a defect,
or interruption. If both the action and cleanup fail, the cleanup outcome wins;
there is no hidden secondary-error channel. `bracket acquire release use`
protects the whole resource lifetime, including the cancellation-sensitive gap
between acquisition and installing release.
Higher-level helpers such as `withFile`, `withTcpConnection`,
`withTcpListener`, `withUdpSocket`, and `withThread` are built around that
ownership model.

The compiler also rejects non-exhaustive patterns before the program runs, so
a missing data case is not turned into an implicit runtime error.

Next: [Functions, modules, and typeclasses](04-abstraction.md).
