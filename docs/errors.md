# Errors, Defects, and Interruption

Solveig keeps three kinds of program failure separate. The distinction decides
whether a failure appears in a type, whether ordinary sequencing can recover
from it, and whether the runtime unwinds the current computation.

| Channel | Examples | Visible in `IO e a` | Recovery |
| --- | --- | --- | --- |
| Typed error | File or terminal failure, a domain error | Yes, as `e` | `catchError`, `mapError`, or `orElse` |
| Defect | `error`, division by zero, managed exhaustion | No | `catchDefect` at an installed IO scope |
| Interruption | A killed thread, timeout, or lost race | No | Not catchable as an ordinary error; cleanup still runs |

Use a typed error when callers are expected to make a normal decision about
the failure. Use a defect for a broken program assumption or an exhausted
runtime resource. Interruption is external cancellation of otherwise valid
work; it is neither a returned domain value nor a defect to suppress.

VM infrastructure failures are outside these three language channels. A VM
trap, cyclic global evaluation, or scheduler deadlock produces an uncatchable
`RuntimeError`. `catchDefect` deliberately cannot turn a compiler or runtime
invariant failure into an ordinary program value.

## Typed errors

`IO e a` describes an action that can either fail with an ordinary value of
type `e` or succeed with a value of type `a`. The Prelude alias
`Task a = IO IOError a` is the conventional type for executable programs.
`IOError` is closed and names the operation that failed:

```solveig
data IOError =
    StdoutError String
  | StdinError String
  | ReadFileError String String
  | WriteFileError String String
  | AppendFileError String String
  | ChannelClosed
```

A typed failure short-circuits `bind`: later actions in the same sequence do
not run. It remains a value rather than unwinding the ordinary call stack.
`catchError` resumes with a handler and may change the error type;
`mapError` translates the error value without changing the success value;
`orElse` discards the first error and runs a fallback.

```solveig
reportIOError : IOError -> Task ()
reportIOError ioError =
    case ioError of
        StdoutError message => printLine (appendString "stdout: " message)
        | StdinError message => printLine (appendString "stdin: " message)
        | ReadFileError path message =>
            printLine (appendString path (appendString ": " message))
        | WriteFileError path message =>
            printLine (appendString path (appendString ": " message))
        | AppendFileError path message =>
            printLine (appendString path (appendString ": " message))
        | ChannelClosed => printLine "channel closed"

printSettings : Task ()
printSettings = do
    contents <- readFile "settings.txt"
    printLine contents

main : Task ()
main = catchError printSettings reportIOError
```

Libraries should expose domain-specific error types when the caller can act on
more useful information than `IOError` alone:

```solveig
data ConfigError = ConfigIO IOError

loadConfig : String -> IO ConfigError String
loadConfig path = mapError ConfigIO (readFile path)
```

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

Interruption-safe installation needs masking so cancellation cannot land
between acquisition and installing the cleanup frame. Phase J2 adds that
mechanism together with thread cancellation. Until then, `finally` and
`bracket` provide their completed guarantees for success, typed failure, and
defects, but no public interruption API exists yet.

## Interruption

Interruption cancels work because another computation requested it: for
example, a future `killThread`, a timeout, or the losing side of a race. It is
delivered only at VM safe points so runtime state remains valid. It is not an
`IOError`, is not converted to `Defect`, and will not be caught by
`catchError` or `catchDefect`.

Phase J defines the public interruption and structured-concurrency operations.
Its masking rule will defer delivery while a cleanup scope is installed or
executed, then restore the previous masking state. Code should use `bracket`
rather than inventing an acquire/use/release sequence so that the same source
program gains the full interruption guarantee when that machinery lands.

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
- If work must be cancelled externally, use the structured interruption API
  introduced by Phase J rather than encoding cancellation as an error value.
- If the VM reports an infrastructure failure, treat it as an implementation
  problem; program handlers cannot catch it.

These distinctions are semantic, not presentation choices. Converting every
failure to the same string would erase which operations are expected to fail,
which computations are being abandoned, and which runtime invariants need
repair.
