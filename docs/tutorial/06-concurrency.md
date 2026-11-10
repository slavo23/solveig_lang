# 8. Make independent work overlap

Release Radar spends far more time waiting for responses than decoding them.
Log Audit has a producer, several file workers, and a collector that must stay
live together. Concurrency lets both applications make progress while one part
waits for timers, files, sockets, channels, or another task. Solveig runs
lightweight language threads on its VM and moves blocking I/O to bounded host
boundaries so another runnable language thread can continue.

Concurrency is not the same as parallel CPU execution. Its most immediate
benefit is overlapping waits. Whether a particular workload becomes faster is
an empirical question; measure the real program rather than assuming.

**Skip ahead:** readers familiar with structured concurrency should study
[bounded concurrent mapping](#bounded-concurrent-mapping) for Release Radar and
[Log Audit's bounded pipeline](#log-audits-bounded-pipeline).

## A useful runtime model

A Solveig green thread is a suspended language computation, not an operating
system thread. The VM keeps a value stack, continuation stack, current
instruction, and interruption state for each green thread. One scheduler OS
thread executes Solveig bytecode from a FIFO run queue. Blocking file and DNS
work goes to a bounded worker pool, while readiness-driven sockets are owned by
the networking event loop. Those host threads return owned data; they never
evaluate Solveig code or inspect VM values.

This gives four important consequences:

1. Thousands of waiting green threads need not imply thousands of OS threads.
2. When one green thread waits on `sleep`, a channel, or supported I/O, the
   scheduler can run another.
3. Two CPU-heavy Solveig expressions do not execute bytecode in parallel. They
   take turns at calls, backward jumps, allocations, and reduction-budget
   yields.
4. Shared immutable values require no synchronization. A `Chan a` is the
   explicit mutable coordination point when threads must exchange values.

Think of the scheduler as repeatedly choosing a runnable action, advancing it
until it finishes, blocks, fails, accepts cancellation, or yields, and then
choosing again. Source code remains ordinary blocking-style `IO`; suspension is
a runtime property rather than an `async` function type.

## Start with structured operations

`std.task.concurrently` runs two actions and returns both results:

```solveig
import std.task(concurrently)
import std.time(milliseconds, sleep)

left : Task String
left = do
    sleep (milliseconds 30)
    pureIO "left"

right : Task Int
right = do
    sleep (milliseconds 10)
    pureIO 42

main : Task ()
main = do
    result <- concurrently left right
    case result of
        (text, number) => printLine (text <> ": " <> show number)
```

The tuple order follows the argument order, not completion order. Stable output
therefore does not prove that execution was sequential.

If either action fails or raises a defect, `concurrently` interrupts its peer,
waits for both child scopes to finish, and propagates the failure. Children do
not outlive the call.

Operationally, `concurrently left right` establishes a small ownership tree:

```text
calling scope
├── left child
└── right child
```

The calling scope returns only after both successful values are available or
after a failure has cancelled the peer and that peer has finished cleanup.
This “no child outlives its owner” rule is structured concurrency. It makes
lifetime visible in the expression that starts the work, rather than relying
on a convention that somebody will eventually kill a raw thread.

## Race and timeout

`race left right` returns the first successful or failed completion as
`Either a b` and cancels the loser:

```solveig
import std.task(race)
import std.time(milliseconds, sleep)

main : Task ()
main = do
    winner <- race
        (do sleep (milliseconds 20); pureIO "timer A")
        (do sleep (milliseconds 50); pureIO "timer B")
    printLine (show winner)
```

`timeout duration action` is the common timer race:

```solveig
maybeValue <- timeout (milliseconds 250) operation
case maybeValue of
    Nothing => printLine "operation timed out"
    | Just value => printLine (show value)
```

An expired timeout interrupts the action. It is not merely a check performed
after the action eventually finishes.

`race` means first *completion*, not first success. If the first child produces
a typed error or a defect, that outcome is propagated and the other child is
cancelled. If an application wants “first success,” it must define how to
retain one failure while waiting for the other result; silently discarding the
first failure would be a different policy.

Cancellation is cooperative but prompt at blocking points and VM safe points.
A host system call that is already executing may need to return before all
host-side cleanup completes. Network and process APIs therefore keep their own
finite deadlines and scoped cleanup rules instead of treating `timeout` as a
way to abandon a live resource.

## Bounded concurrent mapping

For a list of independent operations, use
`mapConcurrentlyBoundedOrdered`. Release Radar's checked client keeps the
worker error type until one explicit mapping:

```solveig
fetchConcurrentObserved :
    Int ->
    String ->
    Maybe String ->
    Cache -> [String] -> Task [ReleaseResult]
fetchConcurrentObserved workers base token cache repositories =
    mapError
        concurrentIOError
        (mapConcurrentlyBoundedOrdered
            workers
            (fetchObserved base token cache)
            repositories)
```

The caller passes `workerCount = 4`. At most four fetch actions are active in
this pool. Each worker prints its own completion immediately, so those lines
show observed completion order. The returned `[ReleaseResult]` follows
`repos.txt` order because the combinator restores input order.

Follow one run conceptually:

1. A feeder places indexed jobs into a bounded channel.
2. Four scoped workers receive jobs. When one waits on HTTP, another can run.
3. Each worker sends its index and outcome to a result channel.
4. The collector stores results by index, then reconstructs input order.
5. Leaving the combinator cancels and joins the feeder and every worker.

The scheduler's execution order, the order in which workers finish, and the
order returned to the caller are three different facts. The ordered mapping
changes only the third; it does not serialize the first two.

Use `mapConcurrentlyBoundedCompletionOrder` when arrival order is itself the
result—for example, progressive UI updates or latency-ranked probes. It has the
same worker bound, typed errors, cancellation, cleanup, and channel backpressure
as the ordered form; only the returned list order differs.

The bound is part of correctness as well as performance. It prevents a list of
ten thousand URLs from immediately creating ten thousand in-flight requests.
Choose it from the capacity of the external service, file descriptor budget,
and workload measurements—not from list length alone.

A practical first bound is usually derived from the bottleneck you control:
the service's published concurrency guidance, a database pool size, or a local
descriptor budget. Increasing it beyond that point can increase queueing,
timeouts, memory use, and rate-limit failures while reducing useful
throughput. Treat the number as an application policy to measure, not a magic
property of the machine's CPU count.

When any operation fails, the pool stops and its scoped workers are cancelled
and joined. The generic result distinguishes `ConcurrentWorker workerError`
from `ConcurrentInfrastructure ioError`. Release Radar explicitly collapses
that distinction because both its workers and `main` use `IOError`. An
invalid non-positive capacity is `ConcurrentInfrastructure (IOError
InvalidArgument ...)`.

### Demonstrate useful overlap

Release Radar's default mode measures the same repository list twice against
the same local service: once with `mapM`, then with the bounded pool. The
fixture derives a 30–199 ms delay from each request's bytes. No public network,
DNS service, account, or rate limiter contributes noise.

```solveig
runLocalAgainst : [String] -> String -> Task ()
runLocalAgainst repositories base = do
    sequential <- measure (fetchSequential base Nothing empty repositories)
    case sequential of
        (sequentialNanos, _) => do
            concurrent <-
                measure
                    (fetchConcurrentObserved
                        workerCount
                        base
                        Nothing
                        empty
                        repositories)
            case concurrent of
                (concurrentNanos, results) =>
                    reportLocalComparison
                        repositories
                        sequentialNanos
                        results
                        concurrentNanos
```

`reportLocalComparison` fails if the concurrent duration is not smaller. This
is an application demonstration under controlled delays, not a general
benchmark claim. It establishes the situation where concurrency wins: many
independent operations spend most of their time waiting and the service allows
four of them to overlap.

For a unit test that should not depend on a stopwatch at all, use channels as a
barrier: require all workers to announce that they reached a waiting point
before releasing any. A sequential traversal cannot satisfy that protocol,
while a pool with the required capacity can. The repository's concurrency
fixtures use channel ordering and deterministic scheduler mode for exact
behavioral assertions; elapsed-time measurement is reserved for the real local
application.

Concurrency does not make dependent steps independent. Loading credentials,
checking access, and then issuing authenticated requests remains ordered by
data dependencies.

A useful review question is therefore not “can this be concurrent?” but:

1. Are the operations independent, so reordering their progress is valid?
2. Is waiting a substantial part of their cost?
3. What external capacity gives the worker bound a principled value?
4. What should happen to peers after one job fails?
5. Must results retain input order, or should consumers see completion order?

## Explicit thread scopes

`withThread child body` starts `child`, passes its `ThreadId` to `body`, and
ensures the child is interrupted and joined when `body` leaves:

```solveig
import std.task(withThread)
import std.time(milliseconds, sleep)

heartbeat : Task ()
heartbeat = do
    printLine "working"
    sleep (milliseconds 100)
    heartbeat

main : Task ()
main =
    withThread heartbeat (fun _ => do
        sleep (milliseconds 250)
        printLine "main work finished")
```

This is the normal primitive when structured helpers are not enough. The raw
`std.thread.spawn` exists, but it does not establish the same lexical ownership
by itself. Prefer `withThread` so lifetime and cleanup remain visible.

`withThread` is for lifetime ownership, not result collection: its body
receives a `ThreadId`, not the child's value. Use `concurrently`, `race`, or a
typed result channel when the parent must observe the child's outcome. A raw
child failure does not automatically become `main`'s typed failure.

## Typed channels

A `Chan a` carries values of one type. Channels provide FIFO message order:

```solveig
import std.channel(*)
import std.task(withThread)

produce : Chan Int -> Task ()
produce output = do
    send output 10
    send output 20
    closeChan output

main : Task ()
main = do
    values <- newChan
    withThread
        (produce values)
        (fun _ => do
            first <- recv values
            second <- recv values
            printLine $ show (first, second))
```

`recv` waits while an open channel is empty. `send` to an unbounded channel
queues immediately. `tryRecv` returns `Nothing` instead of waiting when no value
is ready.

After `closeChan`, queued values remain receivable. Once they are exhausted,
`recv` and `tryRecv` fail with an `IOError` whose `ResourceVanished` kind and
message identify the closed channel; sending to a closed channel fails the
same way. `tryRecv` returns `Nothing` only for an open, currently empty channel.
Closing is useful for protocols where “no more values” is meaningful.

Treat channel use as a protocol with named roles:

- State which values may be sent and what each value means.
- Give one participant responsibility for closing, normally the participant
  that knows no more values can be produced.
- With multiple producers, close only after all producers have finished; an
  individual worker must not close a shared result channel early.
- Decide whether consumers terminate after a known count, a separate sentinel,
  or channel closure.
- Never have every participant wait to receive before any participant can send.

The type `Chan a` ensures message types agree, but it cannot prove these
temporal rules. A well-typed protocol can still deadlock if its ownership and
termination conditions are inconsistent.

## Backpressure with bounded channels

`newBoundedChan capacity` creates a queue with a positive maximum size. A sender
waits when the queue is full, allowing a slower consumer to regulate a faster
producer:

```solveig
jobs <- newBoundedChan 16
```

Backpressure prevents unchecked memory growth and makes overload visible as
waiting. It does not decide how many consumers should exist; worker count and
queue capacity are separate design choices.

An unbounded channel is appropriate only when the producer is already bounded
or the complete queue is intentionally part of the result. If production can
outpace consumption for an unbounded duration, the queue is an unbounded memory
commitment. A bounded channel turns that mismatch into suspension at `send`,
so pressure propagates toward the producer instead of accumulating invisibly.

A useful pipeline usually has:

```text
producer -> bounded jobs channel -> scoped workers -> results channel -> collector
```

Messages may arrive at the collector in completion order. Attach an index when
the final output must reproduce input order. This is exactly the distinction
between a concurrent schedule and a deterministic presentation.

## Log Audit's bounded pipeline

Log Audit uses the explicit primitives because it has a multi-stage protocol,
not merely a list map:

```text
enqueueJobs
    │
    ▼
bounded Chan LogJob (capacity 2)
    │
    ├── worker 1 ─┐
    ├── worker 2 ─┼──▶ bounded Chan AuditResult (capacity 2) ──▶ collectResults
    └── worker 3 ─┘
```

The actual owner is `runAudit`:

```solveig
runAudit : Chan LogJob -> Chan AuditResult -> Task AuditSummary
runAudit jobs results =
    withThread
        (worker jobs results)
        (fun firstWorker =>
            withThread
                (worker jobs results)
                (fun secondWorker =>
                    withThread
                        (worker jobs results)
                        (fun thirdWorker => do
                            completed <-
                                concurrently
                                    (enqueueJobs jobs)
                                    (collectResults results 5 emptySummary)
                            case completed of
                                (produced, summary) => pureIO summary
                        )
                )
        )
```

The three thread identifiers and producer result are intentionally unused; the
scopes are present for ownership. Producer and collector must overlap. If the
program enqueued all five jobs before starting the collector, a capacity-two
jobs queue and capacity-two results queue could form this deadlock:

1. workers fill the results queue;
2. workers stop receiving jobs because they are blocked sending results;
3. the producer blocks because the jobs queue is full;
4. no collector is running to free result capacity.

`concurrently enqueueJobs collectResults` ensures the consumer is live while
the producer and workers can be backpressured. The producer closes only the
jobs channel because it knows when no jobs remain. The collector receives a
known count. `main` closes the results channel after all five outcomes have
been collected.

Results may arrive in any order, but `AuditSummary` is an order-independent
aggregation. Unlike Release Radar, Log Audit does not need to attach indexes and
restore presentation order. Choosing the result algebra carefully can remove
ordering work.

## Failure and cancellation

Solveig keeps three exit mechanisms distinct:

- A typed error is part of `IO e a` and can be handled with `catchError`.
- A defect represents a violated program/runtime assumption and can be
  observed with `catchDefect` when a boundary deliberately needs to report it.
- Interruption is an asynchronous cancellation request. It is consumed by the
  runtime while unwinding and is not accidentally swallowed by either error
  handler.

Blocking operations such as `recv`, a full-channel `send`, `sleep`, socket IO,
and supported I/O are interruption points. When their scope is cancelled,
they wake instead of keeping a parent stuck forever.

Use `finally` or `bracket` for cleanup. `mask` defers interruption during a
small critical region except while an operation is blocked. Most application
code should rely on `withFile`, `withTcpConnection`, `withTcpListener`,
`withUdpSocket`, and `withThread` rather than using `mask` directly.

Never use a channel protocol in which every participant waits to receive before
any participant can send. Concurrency cannot repair a logical deadlock. State
who sends, who closes, and how every loop terminates.

Cleanup installed with `finally` runs after success, typed failure, defects,
and interruption. `bracket acquire release use` closes the cancellation window
between acquiring a resource and installing its release action. Prefer a
domain owner such as `withTcpConnection` or `withFileByteStream` when one
exists: it packages acquisition, rank-2 non-escape, and release into one API.

## Fairness, blocking, and CPU work

FIFO scheduling is deterministic for a fixed program and fixed external
events, but it is not a real-time guarantee. Host completion order belongs to
the operating system. A native primitive runs atomically until it returns to a
safe rooted state. Pure recursive loops yield through the reduction budget,
but splitting a CPU-bound algorithm into many green threads still does not make
it parallel.

Use concurrency when work spends meaningful time waiting or when independent
lifetime management clarifies the program. For parallel CPU throughput, a
future parallel runtime or external worker processes would be a different
execution model; do not infer it from `spawn`.

When every remaining thread is blocked on internal state that no thread can
change, Solveig reports that all threads are blocked forever instead of
spinning. That diagnostic identifies the symptom; the protocol analysis above
is how to find the missing send, close, or terminating branch.

## Deterministic timer tests

With `SOLVEIG_SCHED_DETERMINISTIC=1`, scheduler time is virtual. When no thread
is runnable, it jumps to the next timer deadline, so `sleep` and `timeout` tests
can be exact and fast. Equal deadlines wake in thread-creation order.

Virtual time does not virtualize the operating system. File, DNS, TCP, TLS, and
HTTP completion order remains external. A hermetic concurrency test should use
channels to establish required ordering and a loopback fixture for networking;
it should not assert that a real host request happens to beat a virtual timer.

## Measuring useful concurrency

`std.time.measure action` returns `(elapsedDuration, result)` using a
monotonic clock. Convert explicitly with `durationToNanoseconds` or
`durationToMilliseconds` only where a numeric unit is required. To compare
sequential and concurrent versions:

1. Run the same inputs and the same operation.
2. Keep correctness checks identical.
3. Warm up if startup dominates.
4. Repeat enough times to see variance.
5. Report both latency and resource bounds.

For network programs, use a controlled local service with deliberate response
delays before drawing conclusions from the public internet. Remote caches,
rate limits, and network variation can otherwise overwhelm the effect you want
to measure.

## Deliberate error: removing one concurrent participant

If Log Audit runs `enqueueJobs jobs` to completion before starting
`collectResults`, the program can block forever under its existing capacities.
The types still agree: channel protocols include temporal facts that `Chan a`
cannot express. The runtime's all-threads-blocked diagnostic identifies the
symptom; the ownership diagram identifies the cycle.

## Checkpoint

Run both concurrency designs:

```sh
cargo run -- --project-root examples/log_audit
cargo run -- --project-root examples/release_radar
```

For Log Audit, account for every send, receive, close, and scope. For Release
Radar, compare completion lines with the final ordered report and explain why
they differ.

## Exercises

1. Reduce both Log Audit channel capacities to one in a scratch copy. Explain
   why correct concurrent producer/collector ownership still makes progress.
2. Change Release Radar's worker count to one. Predict the timing relationship
   and completion order.
3. Choose completion-order or input-order mapping for a health dashboard and
   justify the user-visible contract.
4. Design failure policy for three independent downstream calls when one fails:
   cancel peers, keep partial results, or represent every outcome as domain
   data.
5. Explain why green threads improve these waiting workloads but do not execute
   two ray-tracing pixels on two CPU cores.

## Recap

- Solveig schedules green threads on one bytecode-executing OS thread.
- Supported blocking IO suspends the green thread through bounded host
  machinery; it does not freeze the scheduler.
- Structured combinators cancel and join children before returning.
- Worker and queue bounds are correctness and resource policies.
- Ordered results, completion order, and execution schedule are distinct.
- Channels type messages but cannot prove protocol liveness or close ownership.
- Log Audit uses an explicit bounded pipeline; Release Radar uses a bounded map.
- Controlled local delay demonstrates useful overlap without claiming CPU
  parallelism or public-network benchmark results.

Next: [Finish and operate the application](09-packages-and-tooling.md).
