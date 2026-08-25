# Errors, Defects, Interruption, and Exit

Solveig keeps three kinds of program failure separate and models requested
program exit as a fourth control outcome. The distinction decides whether an
outcome appears in a type, whether ordinary sequencing can recover from it,
and whether the runtime unwinds the current computation.

| Channel | Examples | Visible in `IO e a` | Recovery |
| --- | --- | --- | --- |
| Typed error | File or terminal failure, a domain error | Yes, as `e` | `catchError`, `mapError`, or `orElse` |
| Defect | `error`, division by zero, managed exhaustion | No | `catchDefect` at an installed IO scope |
| Interruption | A killed thread, timeout, or lost race | No | Not catchable as an ordinary error; cleanup still runs |
| Program exit | `exitWith`, `exitSuccess`, or `exitFailure` | No | Not catchable as an ordinary error; cleanup still runs |

Use a typed error when callers are expected to make a normal decision about
the failure. Use a defect for a broken program assumption or an exhausted
runtime resource. Interruption is external cancellation of otherwise valid
work; it is neither a returned domain value nor a defect to suppress.
Program exit is a requested control outcome rather than a failure: when it
escapes `main`, the scheduler shuts down sibling work and returns its status to
the host.

VM infrastructure failures are outside these language channels. A VM
trap, cyclic global evaluation, or scheduler deadlock produces an uncatchable
`RuntimeError`. `catchDefect` deliberately cannot turn a compiler or runtime
invariant failure into an ordinary program value.

## Typed errors

`IO e a` describes an action that can either fail with an ordinary value of
type `e` or succeed with a value of type `a`. The Prelude alias
`Task a = IO HostError a` is the convenient type for low-level host actions and
ordinary `main` functions.

`HostError` is deliberately small and stable. Its kind describes the cause,
its operation identifies the primitive boundary, and its message retains host
detail for diagnostics:

```solveig
data HostErrorKind =
    AlreadyExists
  | DoesNotExist
  | PermissionDenied
  | ResourceBusy
  | ResourceExhausted
  | EndOfFile
  | InvalidArgument
  | UnsupportedOperation
  | TimeExpired
  | ResourceVanished
  | Interrupted
  | NetworkUnreachable
  | ConnectionRefused
  | AddressInUse
  | ProtocolError
  | OtherError

data HostError = HostError HostErrorKind String String
```

Use `hostErrorKind`, `hostErrorOperation`, and `hostErrorMessage` when logic
needs one field. Prefer the stable kind for decisions; host message wording can
differ between operating systems. `Show HostError` already produces a useful
diagnostic, so applications do not need exhaustive rendering functions.

A typed failure short-circuits `bind`: later actions in the same sequence do
not run. It remains a value rather than unwinding the ordinary call stack.
`catchError` resumes with a handler and may change the error type;
`mapError` translates the error value without changing the success value;
`orElse` discards the first error and runs a fallback.

```solveig
reportHostError : HostError -> Task ()
reportHostError hostError =
    if hostErrorKind hostError == DoesNotExist then
        printLine "required input does not exist"
    else printLine (show hostError)

printSettings : Task ()
printSettings = do
    contents <- readFile "settings.txt"
    printLine contents

main : Task ()
main = catchError printSettings reportHostError
```

Higher-level libraries expose domain errors rather than growing `HostError`
with one constructor per operation:

```solveig
data ConfigError = ConfigFile FsError | InvalidConfig String

listConfigDirectory : std.path.Path -> IO ConfigError [std.path.Path]
listConfigDirectory path = mapError ConfigFile (std.fs.listDirectory path)
```

The current standard library provides `FsError`, `ProcessError`, `NetError`,
`HttpError`, and `EnvError`. Each has `Eq` and `Show`, preserves useful domain
context, and may wrap the original host cause. HTTP also retains transport,
TLS, redirect, and response-bound policy cases rather than reducing them to a
generic protocol failure. Strict UTF-8 decoding is pure
and uses `Utf8Error` (`InvalidUtf8 byteOffset`) rather than pretending that a
decoding problem is host IO.

Bounded worker traversal also preserves the distinction:

```solveig
data ConcurrentError e =
    ConcurrentInfrastructure HostError
  | ConcurrentWorker e

mapConcurrentlyBounded :
    Int -> (a -> IO e b) -> [a] -> IO (ConcurrentError e) [b]
```

For workers already using `Task`, `concurrentHostError` explicitly collapses
both cases back to `HostError` when that loss of distinction is appropriate.

Do not use a defect for a missing file, invalid request, failed lookup, or
another expected outcome. Giving that outcome a type makes recovery visible to
the caller and keeps ordinary control flow explicit.

## Defects

The closed `Defect` type classifies failures that are not part of an action's
typed contract:

```solveig
data Defect =
    Error String
  | DivisionByZero
  | StackExhausted
  | HeapExhausted
```

`error message` raises `Error message`. Integer `div` and `mod` raise
`DivisionByZero` for a zero divisor. The VM raises `StackExhausted` or
`HeapExhausted` before exceeding its managed process-stack or live-object
ceiling. Heap exhaustion is reported only after a full collection confirms
that the retained live set is still at the ceiling.

A defect unwinds calls, IO binds, and typed-error handlers until it finds an
installed `catchDefect` scope. `catchError` cannot catch it. If no defect scope
exists, the current thread fails; an uncaught defect in a spawned thread is
reported for that thread and does not become the parent's typed error.

```solveig
reportDefect : Defect -> Task ()
reportDefect defect =
    case defect of
        Error message => printLine (appendString "error: " message)
        | DivisionByZero => printLine "division by zero"
        | StackExhausted => printLine "stack exhausted"
        | HeapExhausted => printLine "heap exhausted"

divide : Task ()
divide =
    bindIO
        (pureIO ())
        (fun ignored => printLine (show (div 10 0)))

main : Task ()
main = catchDefect divide reportDefect
```

The callback in `divide` is significant. Solveig is strict, so the arguments
to `catchDefect` are evaluated before its IO scope is installed. This does not
catch the division defect:

```solveig
main = catchDefect (printLine (show (div 10 0))) reportDefect
```

The division occurs while constructing the protected action. Put pure work
that may defect inside an executed `bind`, `use`, or other IO callback so it
runs after the scope has been installed.

`catchDefect` is intended for a deliberate recovery boundary, such as logging
and isolating one top-level job. It should not replace typed errors throughout
an API: doing so removes failure information from signatures and makes local
reasoning harder.

## Cleanup

`finally action cleanup` runs `cleanup` after `action` succeeds, returns a
typed error, or raises a defect. `bracket acquire release use` acquires a
resource and installs `release resource` before running `use resource`.

```solveig
release : String -> Task ()
release resource = printLine (appendString "release " resource)

use : String -> Task ()
use resource =
    bindIO
        (printLine (appendString "use " resource))
        (fun ignored => printLine (show (div 1 0)))

main : Task ()
main =
    catchDefect
        (bracket (pureIO "resource") release use)
        reportDefect
```

Here `release resource` runs before `reportDefect`. If cleanup itself returns a
typed error or raises a defect, that cleanup failure replaces the saved
outcome. This prevents a failed release from being hidden.

Interruption-safe installation uses masking so cancellation cannot land between
acquisition and installing the cleanup frame. `bracket` masks acquisition and
cleanup, restores the caller's interruption state for the use action, and runs
release while unwinding an interruption. `finally` applies the same rule to an
action and cleanup pair.

## Interruption

Interruption cancels work because another computation requested it: for
example, `killThread`, `timeout`, or the losing side of `race`.
It is delivered only at VM safe points so runtime state remains valid. It is
not a typed error, is not converted to `Defect`, and will not be caught by
`catchError` or `catchDefect`.

`mask action` defers delivery while `action` is runnable, then restores the
previous nesting depth and delivers any pending request. A channel or worker-IO
operation that must actually wait remains interruptible even inside `mask`.
This makes acquisition atomic without making indefinite waits unkillable.
Cleanup entered because of interruption runs masked; a second interruption may
still stop cleanup if it reaches an interruptible blocking operation. Solveig
does not expose an uninterruptible mask because `bracket` does not require one
and permanently unkillable waits would be a worse default.

Structured combinators propagate a child's typed error with its original error
type and re-raise an observed defect with its original `Defect` constructor.
Cancellation of a race loser or timed-out action remains interruption, not a
typed error. Scope cleanup waits for the cancelled child to finish unwinding,
so no child survives its `withThread`, `race`, `concurrently`, or `timeout`
scope.

## Program exit

`exitWith status` uses the same cleanup stack as interruption but remains a
distinct synchronous outcome. `catchError` and `catchDefect` ignore it;
`finally` and `bracket` run cleanup. If cleanup itself fails or raises a defect,
that newer outcome replaces the exit request.

Only an exit escaping `main` terminates the program. The scheduler interrupts
each live sibling once, lets its installed cleanup finish, flushes worker-owned
handles during runtime shutdown, and returns the requested status to the host.
An exit escaping a spawned thread retires that thread without changing the
main program's status.

## Pattern-match failures are compile-time errors

An incomplete pattern match is not a fourth runtime failure channel. The type
checker reports `TC0016` before bytecode is produced:

```solveig
bad : Bool -> Int
bad value =
    case value of
        True => 1
```

This definition omits `False`, so it does not compile. Conversely, an empty
match on `Never` is exhaustive because `Never` has no constructors. No hidden
runtime match exception is needed for either case.

## Choosing a channel

- If a caller can reasonably recover, return a typed error.
- If a program invariant is broken, raise or allow a defect and catch it only
  at a deliberate isolation boundary.
- If work must be cancelled externally, use structured interruption rather
  than encoding cancellation as an error value.
- If the whole program should terminate with a status, request program exit
  from `main`; do not encode it as a typed error or defect.
- If the VM reports an infrastructure failure, treat it as an implementation
  problem; program handlers cannot catch it.

These distinctions are semantic, not presentation choices. Converting every
failure to the same string would erase which operations are expected to fail,
which computations are being abandoned, and which runtime invariants need
repair.
