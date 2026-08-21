# Concurrency

Solveig uses blocking-style concurrency over lightweight green threads. Code
is written as ordinary `IO e a` actions: there are no futures, `async`
functions, or function-colouring rules. A thread can block on a channel, timer,
file operation, DNS request, or socket while other Solveig threads continue.

Only one OS thread executes Solveig bytecode. Every green thread shares the VM
heap and global table, but owns its own value stack, continuation stack,
instruction state, reduction budget, and interruption state. Blocking host IO
runs on a small worker pool, and one additional event-loop thread owns all TCP
sockets. Those host threads exchange owned Rust data with the scheduler; they
never evaluate Solveig code or access VM values. This is concurrency, not
parallel execution of Solveig programs.

Concurrency capabilities are explicit standard-library imports. A typical
networked program starts with the modules it actually needs:

```solveig
import std.channel
import std.thread
import std.task
import std.time
import std.bytes
import std.net
```

The design follows the lightweight-thread and typed-channel model of Peyton
Jones, Gordon, and Finne, “Concurrent Haskell” (POPL 1996). Interruption and
masking follow Marlow, Peyton Jones, Moran, and Reppy, “Asynchronous Exceptions
in Haskell” (PLDI 2001). The structured combinator shapes follow Haskell's
`Control.Concurrent.Async`, adapted to Solveig's typed-error model.

## Runtime boundary

The runtime implements only operations that need scheduler, heap, clock, or
host-resource access. Higher-level lifetime policy remains ordinary
standard-library code.

| Layer | Operations | Responsibility |
|---|---|---|
| Public runtime-backed operations | `spawn`, `killThread`, `mask`, `sleep`, `newChan`, `newBoundedChan`, `send`, `recv`, `tryRecv`, `closeChan` | Create or suspend processes, deliver interruption, and mutate channel state. |
| Network runtime-backed operations | `resolveHost`, `connect`, `listen`, `accept`, `sendBytes`, `recvBytes`, `closeSocket` | Cross the host DNS and TCP boundary without blocking the scheduler. |
| Private runtime primitives in `prelude.prim` | `maskIO`, `restoreMaskIO`, `finallyIO`, `raiseDefectIO` | Preserve mask state and exact outcomes while public cleanup/combinator code is expressed in Solveig. |
| Public `std.task` library | `finally`, `bracket`, `withThread`, `race`, `concurrently`, `timeout` | Define cleanup, child lifetime, result propagation, and cancellation policy using the primitives above. |

`MVar`, semaphores, actor mailboxes, supervision trees, STM, and async/await are
not separate features. Typed channels plus structured scopes are the intended
coordination surface.

## Green threads and scheduling

`spawn : forall childError e. IO childError () -> IO e ThreadId` starts an
action and returns an opaque thread identity. The new process joins the back of
a strict FIFO run queue. A process runs until it:

- reaches its reduction limit and yields;
- blocks on one typed resource;
- finishes;
- fails; or
- accepts an interruption.

Calls, taken backward jumps, and allocations are safe points. They consume the
same reduction budget, whose default is 4,096. A pure loop therefore cannot
hold the VM indefinitely: its backward jump eventually returns control to the
scheduler. `SOLVEIG_REDUCTIONS=<positive u32>` changes the quantum for testing
or measurement.

When a process yields, it rejoins the FIFO tail. A channel or timer wakeup also
appends the awakened process to that tail. This gives stable scheduling for a
fixed program and fixed external inputs, but it is not a general real-time or
starvation-freedom guarantee. A native builtin is atomic until it restores its
rooted state, and host IO completion order is determined by the operating
system.

Raw `spawn` is intentionally low-level. Main-process completion ends the
program even if raw children remain. A failure in an unstructured non-main
thread is reported to stderr and does not automatically fail main. Prefer
`withThread`, `race`, or `concurrently` when a child's lifetime or result
matters.

When the run queue is empty, the scheduler waits only if a timer or outstanding
host request can still make progress. If every remaining process is blocked on
internal state that no process can change, execution reports `all threads are
blocked forever` instead of spinning.

## Typed channels

```solveig
newChan : forall e a. IO e (Chan a)
newBoundedChan : forall a. Int -> Task (Chan a)
send : forall a. Chan a -> a -> Task ()
recv : forall a. Chan a -> Task a
tryRecv : forall a. Chan a -> Task (Maybe a)
closeChan : forall a. Chan a -> Task ()
```

`Chan a` is an abstract, mutable heap object shared through ordinary values.
Its queued values are FIFO. Sender and receiver wait queues are separate FIFO
queues owned by the scheduler, not stored inside the channel object.

- `newChan` creates an unbounded queue.
- `newBoundedChan capacity` requires a value from 1 through `u32::MAX`.
  Invalid capacity raises the typed `InvalidChannelCapacity capacity` error;
  capacity zero is not a rendezvous channel.
- `send` hands its value directly to the oldest waiting receiver, otherwise
  enqueues it. On a full bounded channel it parks the sender.
- `recv` removes the oldest queued value. On an open empty channel it parks the
  receiver. Removing from a full bounded channel admits the oldest waiting
  sender.
- `tryRecv` never parks. It returns `Nothing` only for an open empty channel.
- `closeChan` is idempotent. Already queued values remain readable. After they
  are drained, receives fail with `ChannelClosed`; sends fail immediately.
  Closing also wakes blocked senders and receivers with `ChannelClosed`.

Channel mutation is the first old-to-young heap edge in Solveig. The runtime
applies the generational write barrier when queueing a value, traces queued
values, and treats parked sender values as scheduler roots. The same invariants
are exercised under GC stress.

A bounded producer/consumer pair can stay entirely in blocking style:

```solveig
produce : Chan Int -> Task ()
produce output = do
    send output 20
    send output 22
    closeChan output

consumeTwo : Chan Int -> Task Int
consumeTwo input = do
    first <- recv input
    second <- recv input
    pureIO (first + second)

main : Task ()
main = do
    values <- newBoundedChan 1
    result <- concurrently (produce values) (consumeTwo values)
    case result of
        (produced, total) => printLine (show total)
```

The first send fills the queue; the second may park until the consumer makes
space. No callback or polling loop is needed.

## Interruption, masking, and cleanup

`killThread : ThreadId -> Task ()` requests interruption and returns without
waiting for delivery. Killing an unknown or already completed thread succeeds.
A runnable target accepts the request at its next safe point. A target blocked
on a channel, timer, global thunk, host IO request, or socket has that exact
registration removed or cancelled before it is made runnable to unwind.

Interruption is distinct from both typed errors and defects. `catchError` and
`catchDefect` do not consume it. Unwinding runs installed `finally` cleanup and
then retires the target process.

`mask : forall e a. IO e a -> IO e a` defers interruption while its action is
runnable. Masks are scoped and nest by depth. Returning, failing, defecting, or
unwinding restores the previous depth. Blocking operations remain
interruptible even while masked; Solveig deliberately exposes no
uninterruptible mask that could make an indefinitely blocked cleanup
unkillable.

`finally` and `bracket` use private mask-state primitives so resource
acquisition and cleanup installation have no cancellation window:

```solveig
finally : forall e a b. IO e a -> IO e b -> IO e a
bracket : forall e resource cleanup a.
    IO e resource ->
    (resource -> IO e cleanup) ->
    (resource -> IO e a) ->
    IO e a
```

The protected body runs with the caller's interruption state restored.
Cleanup runs masked after success, typed failure, defect, or interruption. A
cleanup typed failure or defect replaces the saved outcome. A later
interruption can still stop cleanup if cleanup blocks.

Use `bracket` for channels, sockets, and handles. The GC
does not own external-resource lifetime, and `closeSocket` is explicit and
idempotent.

## Structured concurrency

```solveig
withThread : forall e a b. IO e a -> (ThreadId -> IO e b) -> IO e b
race : forall e a b. IO e a -> IO e b -> IO e (Either a b)
concurrently : forall e a b. IO e a -> IO e b -> IO e (a, b)
timeout : forall e a. Int -> IO e a -> IO e (Maybe a)
```

These functions are implemented in `std.task` using Solveig, `bracket`,
`killThread`, and private typed channels:

- `withThread child body` starts `child`, runs `body` with its identity, and
  interrupts the child on every body exit path. It waits for child cleanup
  before leaving the scope.
- `race left right` returns the first completed value as `Left value` or
  `Right value` and interrupts the loser. If the first outcome is a typed error
  or defect, it propagates that exact value and cancels the peer.
- `concurrently left right` returns `(leftValue, rightValue)` after both
  succeed. The first typed error or defect cancels the other action and
  propagates unchanged.
- `timeout milliseconds action` races a timer against the action, returning
  `Nothing` for the timer or `Just value` for successful completion.

The central lifetime rule is: **a child created by a structured combinator
never outlives that combinator's scope**. Cancellation is not merely requested;
the scope waits until the child has run its installed cleanup.

## Timers and deterministic time

`sleep : Int -> Task ()` parks only the current green thread. Non-positive
durations complete immediately. Positive durations register a monotonic
millisecond deadline in the scheduler. Equal deadlines wake in process-id
order, which is process-creation order.

Normal CLI execution uses monotonic host time. With
`SOLVEIG_SCHED_DETERMINISTIC=1`, the runtime instead uses virtual time: when no
process is runnable, the logical clock jumps to the next deadline. Timer and
timeout tests therefore finish immediately and reproducibly, without sleeping
the test process.

Virtual time controls scheduler timers, not the operating system. It does not
make file, DNS, or socket completion order deterministic. In particular, a
virtual deadline can win as soon as all Solveig processes are blocked even if
real IO would complete shortly afterward. Tests that combine timers with host
IO must arrange an explicit channel gate or avoid asserting their relative
winner. The downloader's hermetic `--local` mode therefore does not wrap its
loopback requests in the real-network timeout.

## Blocking IO and TCP

Terminal operations preserve submission order through one blocking worker.
File and DNS requests are distributed across three other workers and may
complete out of order. A blocked process holds a typed `RequestId`; a
completion can wake only the process that still owns that request. Cancelling
a blocking request leaves a tombstone until its worker finishes, so stale
completion data cannot wake a different process.

TCP does not consume those four workers. One `mio` event-loop thread owns every
listener and stream in a monotonic `SocketId` side table. It records pending
accept, connect, send, and receive operations and reports readiness through the
same scheduler completion path. Many green threads can therefore wait on
different sockets without exhausting a thread pool.

```solveig
resolveHost : String -> Task String
connect : String -> Int -> Task Socket
listen : Int -> Task (Socket, Int)
accept : Socket -> Task Socket
sendBytes : Socket -> Bytes -> Task ()
recvBytes : Socket -> Int -> Task Bytes
closeSocket : Socket -> Task ()
closeListener : (Socket, Int) -> Task ()
withConnection : String -> Int -> (Socket -> Task a) -> Task a
withListener : Int -> ((Socket, Int) -> Task a) -> Task a
withAcceptedConnection : Socket -> (Socket -> Task a) -> Task a
```

`resolveHost` performs blocking standard-library DNS on the worker pool and
returns one IPv4 literal. `connect` accepts that literal or `localhost`; it
does not perform DNS itself. `listen` binds IPv4 loopback, and port zero asks
the OS to choose a free port returned as `(listener, actualPort)`.

Prefer `withConnection`, `withListener`, and `withAcceptedConnection` for
owned sockets. They use `bracket`, so the resource closes after success, a
typed error, a defect, or interruption. `closeListener` closes the socket in
the tuple returned by `listen`; raw `closeSocket` remains available when
ownership must cross a larger scope.

`sendBytes` completes after the whole buffer is sent. `recvBytes` returns at
most its non-negative maximum and returns empty bytes at EOF. Socket values
contain only opaque identities: descriptors never live in the moving heap, GC
never closes sockets, and ids are not reused. Use `bracket` or `finally` with
`closeSocket`.

The low-level surface is TCP plus basic IPv4 resolution. `std.http` adds bounded
HTTP/1.1, chunked decoding, redirects, and platform-verified HTTPS on the
blocking worker pool. UDP and general IPv6 selection are not provided.

The [networking tutorial](tutorial/07-networking-and-http.md) combines a local
TCP service with bounded concurrent HTTP clients. It also explains why stable
input-order results do not imply sequential execution.

## Determinism and fairness guarantees

Solveig guarantees the following for fixed internal inputs:

- runnable processes are selected FIFO;
- a yielded or awakened process rejoins the run-queue tail;
- queued channel values are FIFO;
- blocked channel senders and receivers are resumed FIFO within their own
  queues;
- equal timer deadlines wake by process id;
- deterministic mode uses virtual rather than wall-clock time.

Solveig does not guarantee:

- parallel execution of Solveig bytecode;
- a real-time scheduling deadline;
- deterministic order among independent host IO or socket completions;
- fairness from a native builtin that never returns to a safe point; or
- survival of raw spawned threads after main completes.

Prefer explicit channels and structured combinators when observable output
depends on ordering. Do not use elapsed-time guesses as synchronization.

## Testing concurrent programs

Test protocols through exact events rather than timing luck:

1. Coordinate tasks with typed channels or explicit handshakes.
2. Assert conserved values or a final ordered result, not one accidental
   interleaving.
3. Use `sleep` only when timer behavior is what the test is checking.
4. Use a loopback listener on port zero for networking tests instead of public
   DNS or internet access.
5. Close channels and resources explicitly so every blocked loop has a defined
   termination path.
6. Test failure and cancellation as well as successful completion.

`SOLVEIG_SCHED_DETERMINISTIC=1` selects the runtime's deterministic scheduler
mode when a test needs reproducible scheduling. It does not turn a racy protocol
into a correct one; messages and ownership should still define the program's
observable result.

The [VM implementation reference](vm.md) documents scheduler, heap, and host
boundary details for compiler contributors.
