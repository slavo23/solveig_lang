# 3. Model valid states with data and patterns

A missing log is expected in Log Audit; a permission failure is not. If both
were represented by a magic count or empty string, the rest of the program
could not tell them apart safely. Solveig uses algebraic data types and pattern
matching to encode that distinction.

**Skip ahead:** if sums, products, `Maybe`, and `Either` are familiar, read
[typed effects](#typed-effects-are-a-different-layer) and continue to IO.

## Products give values named structure

The application defines:

```solveig
data LogJob = LogJob Path

data LogCounts = LogCounts Int Int Int Int

data AuditSummary = AuditSummary Int Int Int Int Int Int
```

`LogCounts` has one constructor with four fields. It is a product: every value
contains an event, info, warning, and error count. Construction is ordinary
function application:

```solveig
LogCounts 14 8 3 3
```

A pattern names fields:

```solveig
includeResult (AuditSummary files missing events infos warnings errors) result =
    ...
```

Solveig currently has constructor-based product types, not record field syntax.
Field meaning is carried by the type, constructor, patterns, and accessor
functions you choose to define. The tutorial does not use hypothetical record
syntax.

## Sums make alternatives explicit

```solveig
data AuditResult = Audited LogCounts | MissingLog
```

An `AuditResult` is exactly one of:

- `Audited counts`, carrying successful analysis; or
- `MissingLog`, carrying no fields.

There is no third “partly initialized” state. Code that consumes the value must
decide what each constructor means:

```solveig
includeResult : AuditSummary -> AuditResult -> AuditSummary
includeResult (AuditSummary files missing events infos warnings errors) result =
    case result of
        MissingLog => AuditSummary files (missing + 1) events infos warnings errors
        | Audited (LogCounts fileEvents fileInfos fileWarnings fileErrors) =>
            AuditSummary
                (files + 1)
                missing
                (events + fileEvents)
                (infos + fileInfos)
                (warnings + fileWarnings)
                (errors + fileErrors)
```

Nested patterns unpack `Audited` and `LogCounts` at once. The compiler checks
constructor names, field counts, and exhaustiveness.

If you add `UnreadableLog String` to `AuditResult`, every exhaustive match
that has no wildcard becomes a compiler-guided list of places where policy is
missing. This is one reason to prefer domain constructors over booleans and
sentinel strings.

## Patterns can define equations

The same destructuring may appear on the left side of a definition:

```solveig
auditOne : Chan AuditResult -> LogJob -> Task ()
auditOne results (LogJob path) =
    catchError
        (auditFile results path)
        (fun error =>
            if ioErrorOperation error == "readFile" then send results MissingLog else failIO error
        )
```

The `LogJob` wrapper prevents a channel of work from being confused with some
unrelated channel of paths. This is a semantic distinction with no mutable
object behind it.

## `Maybe` models optional success data

The Prelude defines the familiar shape:

```solveig
data Maybe a = Nothing | Just a
```

Release Radar uses `Maybe String` for an optional token and optional response
headers:

```solveig
tokenHeader : Maybe String -> [(String, String)] -> [(String, String)]
tokenHeader Nothing headers = headers
tokenHeader (Just token) headers = ("Authorization", "Bearer " <> token) :: headers
```

`Maybe` says absence is normal data. It does not explain failure. Use it for a
header that may be absent, not for a network request that can fail in several
operational ways.

## `Either` models pure validation failure

`pathFromString : String -> Either PathError Path` validates a path without
performing IO. Log Audit turns its fixed, developer-owned literals into paths:

```solveig
requiredPath : String -> Path
requiredPath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error (show pathError)
```

`Left` is a value-level failure; `Right` is success. Here an invalid literal
is a programmer defect, so the application deliberately calls `error`. For
user input, returning or displaying the `PathError` would be the appropriate
policy.

This separation matters:

- `Maybe a`: optional value, no explanation required;
- `Either e a`: pure computation with an explicit failure value;
- `IO e a`: effectful computation that may fail while executing;
- `error`: unrecoverable defect, not routine control flow.

## Typed effects are a different layer

`std.fs.readFile` has a filesystem-specific error. The application maps it to
its public `Task` boundary:

```solveig
fsToIO : FsError -> IOError
fsToIO (FsIOError _ ioError) = ioError
fsToIO error = IOError InvalidArgument "filesystem" (show error)

auditFile : Chan AuditResult -> Path -> Task ()
auditFile results path = do
    contents <- mapError fsToIO (std.fs.readFile path)
    send results (Audited (analyzeLog contents))
```

`mapError` changes only the error channel. It does not catch defects, erase
the success type, or convert the action to a pure value.

`catchError action handler` handles a typed failure from `action`. In
`auditOne`, a failed `readFile` operation becomes `MissingLog`; other
failures are re-raised with `failIO`. This is intentionally narrow. Swallowing
every `IOError` would misreport permission problems and resource exhaustion as
an absent file.

The current implementation exposes `ioErrorKind`, `ioErrorOperation`, and
`ioErrorMessage`. Prefer structured fields such as kind where the source API
preserves them. Log Audit's operation check is application code, not a general
recommendation to parse human-readable messages.

## Exhaustiveness and fallback patterns

Release Radar maps many HTTP outcomes:

```solveig
renderResult : ReleaseResult -> String
renderResult (ReleaseFound repository tag published url _) =
    repository <> "  " <> tag <> "  " <> published <> "\n  " <> url
renderResult (ReleaseUnchanged repository _) = repository <> "  unchanged (HTTP 304)"
renderResult (ReleaseFailed repository failure) =
    repository <> "  ERROR: " <> describeFailure failure
```

All constructors are visible. Elsewhere, decoding an open-ended JSON value uses
a fallback:

```solveig
releaseFromJson repository _ _ =
    ReleaseFailed repository (InvalidReleaseJson "release JSON is not an object")
```

Use a wildcard when all remaining shapes truly share policy. Avoid it merely to
silence coverage checking on a closed domain type; explicit constructors make
future changes safer.

## A deliberate modeling error

Suppose `includeResult` handles only `Audited`:

```solveig
case result of
    Audited counts => ...
```

The compiler rejects the non-exhaustive match because `MissingLog` is still a
possible value. Adding a wildcard would compile, but `MissingLog => ...`
documents the intended behavior and remains informative if another constructor
is added later.

## Cleanup is not a constructor case

Data modeling cannot by itself guarantee resource release. Handles, sockets,
listeners, and child processes are scoped by APIs such as `withFile`,
`withTcpConnection`, and `withProcess`. Their rank-2 callback types prevent
the scoped resource from escaping. The next chapter explains that boundary;
do not encode an open handle as `Maybe Handle` and hope every branch closes
it.

## Checkpoint

Read `includeResult`, then calculate:

```text
emptySummary
  |> include one MissingLog
  |> include Audited (LogCounts 3 1 1 1)
```

The result is `AuditSummary 1 1 3 1 1 1`. Run the application and identify
which report line corresponds to each field.

## Exercises

1. Add `UnreadableLog String` in a scratch copy and let exhaustiveness errors
   locate the policies you must choose.
2. Explain why an absent `ETag` is `Maybe String`, while an HTTP transport
   failure becomes `ReleaseFailed`.
3. Change `requiredPath` to return `Either PathError Path` and describe how
   that choice would propagate to callers.
4. Find one wildcard in Release Radar and justify why its remaining cases have
   the same behavior.

## Recap

- Product constructors group fields that exist together.
- Sum constructors enumerate valid alternatives.
- Patterns unpack data and are checked for shape and coverage.
- `Maybe`, `Either`, typed `IO`, and defects represent different kinds of
  absence or failure.
- Handle only the failures your application can interpret; propagate the rest.
- Scoped resources require lifetime-aware APIs, not only a data constructor.

Next: [Cross the IO boundary](05-io-and-files.md).
