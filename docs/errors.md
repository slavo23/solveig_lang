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
`Task a = IO IOError a` is the conventional type for executable programs.
`IOError` is closed and names the operation that failed. `ReadFileError` also
reports an invalid or exceeded limit from `readFileLimited` and
`readFileBytesLimited`; `RunProcessError` likewise reports an invalid or
exceeded `runProcessLimited` capture bound. `InvalidChannelCapacity` reports
the rejected value before a bounded channel is allocated:

```solveig
data IOError =
    StdoutError String
  | StdinError String
  | ReadFileError String String
  | WriteFileError String String
  | AppendFileError String String
  | ChannelClosed
  | InvalidChannelCapacity Int
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
  | GetEnvError String
  | SetEnvError String
  | GetCurrentDirectoryError String
  | SetCurrentDirectoryError String String
  | DoesFileExistError String String
  | DoesDirectoryExistError String String
  | ListDirectoryError String String
  | CreateDirectoryError String String
  | RemoveFileError String String
  | RenameFileError String String String
  | GetFileSizeError String String
  | GetModificationTimeError String String
  | RunProcessError String String
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
        | InvalidChannelCapacity capacity =>
            printLine (appendString "invalid channel capacity: " (show capacity))
        | ResolveHostError message =>
            printLine (appendString "host resolution: " message)
        | InvalidUtf8 message => printLine (appendString "invalid UTF-8: " message)
        | ConnectError message => printLine (appendString "connect: " message)
        | ListenError message => printLine (appendString "listen: " message)
        | AcceptError message => printLine (appendString "accept: " message)
        | SendError message => printLine (appendString "send: " message)
        | RecvError message => printLine (appendString "receive: " message)
        | CloseSocketError message => printLine (appendString "close: " message)
        | OpenHandleError path message =>
            printLine (appendString path (appendString ": " message))
        | CloseHandleError message => printLine (appendString "close handle: " message)
        | FlushHandleError message => printLine (appendString "flush handle: " message)
        | ReadHandleError message => printLine (appendString "read handle: " message)
        | WriteHandleError message => printLine (appendString "write handle: " message)
        | SetBufferingError message => printLine (appendString "buffering: " message)
        | GetEnvError message => printLine (appendString "environment: " message)
        | SetEnvError message => printLine (appendString "environment: " message)
        | GetCurrentDirectoryError message => printLine (appendString "cwd: " message)
        | SetCurrentDirectoryError path message =>
            printLine (appendString path (appendString ": " message))
        | DoesFileExistError path message =>
            printLine (appendString path (appendString ": " message))
        | DoesDirectoryExistError path message =>
            printLine (appendString path (appendString ": " message))
        | ListDirectoryError path message =>
            printLine (appendString path (appendString ": " message))
        | CreateDirectoryError path message =>
            printLine (appendString path (appendString ": " message))
        | RemoveFileError path message =>
            printLine (appendString path (appendString ": " message))
        | RenameFileError source destination message =>
            printLine (appendString source (appendString " -> " (appendString destination (appendString ": " message))))
        | GetFileSizeError path message =>
            printLine (appendString path (appendString ": " message))
        | GetModificationTimeError path message =>
            printLine (appendString path (appendString ": " message))
        | RunProcessError program message =>
            printLine (appendString program (appendString ": " message))

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

Interruption-safe installation uses masking so cancellation cannot land between
acquisition and installing the cleanup frame. `bracket` masks acquisition and
cleanup, restores the caller's interruption state for the use action, and runs
release while unwinding an interruption. `finally` applies the same rule to an
action and cleanup pair.

## Interruption

Interruption cancels work because another computation requested it: for
example, `killThread`, `timeout`, or the losing side of `race`.
It is delivered only at VM safe points so runtime state remains valid. It is
not an `IOError`, is not converted to `Defect`, and will not be caught by
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
