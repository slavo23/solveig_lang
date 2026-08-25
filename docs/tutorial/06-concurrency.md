# 6. Concurrency

Concurrency lets one program make progress on multiple tasks while some are
waiting for timers, files, sockets, or other tasks. Solveig runs lightweight
language threads on its VM and moves blocking host IO to a bounded worker
boundary so another runnable language thread can continue.

Concurrency is not the same as parallel CPU execution. Its most immediate
benefit is overlapping waits. Whether a particular workload becomes faster is
an empirical question; measure the real program rather than assuming.

## Start with structured operations

`std.task.concurrently` runs two actions and returns both results:

```solveig
import std.task(concurrently)
import std.time(sleep)

left : Task String
left = do
    sleep 30
    pureIO "left"

right : Task Int
right = do
    sleep 10
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

## Race and timeout

`race left right` returns the first successful or failed completion as
`Either a b` and cancels the loser:

```solveig
import std.task(race)
import std.time(sleep)

main : Task ()
main = do
    winner <- race
        (do sleep 20; pureIO "timer A")
        (do sleep 50; pureIO "timer B")
    printLine (show winner)
```

`timeout milliseconds action` is the common timer race:

```solveig
maybeValue <- timeout 250 operation
case maybeValue of
    Nothing => printLine "operation timed out"
    | Just value => printLine (show value)
```

An expired timeout interrupts the action. It is not merely a check performed
after the action eventually finishes.

## Bounded concurrent mapping

For a list of independent `Task` operations, use
`mapConcurrentlyBounded`:

```solveig
import std.task(mapConcurrentlyBounded)
import std.time(sleep)

process : Int -> Task Int
process value = do
    sleep ((5 - value) * 10)
    pureIO (value * value)

main : Task ()
main = do
    results <- mapConcurrentlyBounded 3 process [1, 2, 3, 4]
    printLine ("results: " <> show results)
```

This complete program is in [`concurrent_jobs`](code/concurrent_jobs/). Three
workers may be active at once. The larger-numbered jobs deliberately wait less,
but the result is still `[1, 4, 9, 16]` because the combinator restores input
order.

The bound is part of correctness as well as performance. It prevents a list of
ten thousand URLs from immediately creating ten thousand in-flight requests.
Choose it from the capacity of the external service, file descriptor budget,
and workload measurements—not from list length alone.

When any operation fails, the pool stops and its scoped workers are cancelled
and joined. An invalid non-positive capacity fails with
`InvalidChannelCapacity`.

## Explicit thread scopes

`withThread child body` starts `child`, passes its `ThreadId` to `body`, and
ensures the child is interrupted and joined when `body` leaves:

```solveig
import std.task(withThread)
import std.time(sleep)

heartbeat : Task ()
heartbeat = do
    printLine "working"
    sleep 100
    heartbeat

main : Task ()
main =
    withThread heartbeat (fun _ => do
        sleep 250
        printLine "main work finished")
```

This is the normal primitive when structured helpers are not enough. The raw
`std.thread.spawn` exists, but it does not establish the same lexical ownership
by itself. Prefer `withThread` so lifetime and cleanup remain visible.

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
            printLine (show (first, second)))
```

`recv` waits while an open channel is empty. `send` to an unbounded channel
queues immediately. `tryRecv` returns `Nothing` instead of waiting when no value
is ready.

After `closeChan`, queued values remain receivable. Once they are exhausted,
`recv` fails with `ChannelClosed`; sending to a closed channel fails the same
way. Closing is useful for protocols where “no more values” is meaningful.

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

A useful pipeline usually has:

```text
producer -> bounded jobs channel -> scoped workers -> results channel -> collector
```

Messages may arrive at the collector in completion order. Attach an index when
the final output must reproduce input order. This is exactly the distinction
between a concurrent schedule and a deterministic presentation.

## Failure and cancellation

Blocking operations such as `recv`, a full-channel `send`, `sleep`, socket IO,
and supported host IO are interruption points. When their scope is cancelled,
they wake instead of keeping a parent stuck forever.

Use `finally` or `bracket` for cleanup. `mask` defers interruption during a
small critical region except while an operation is blocked. Most application
code should rely on `withFile`, `withConnection`, `withListener`, and
`withThread` rather than using `mask` directly.

Never use a channel protocol in which every participant waits to receive before
any participant can send. Concurrency cannot repair a logical deadlock. State
who sends, who closes, and how every loop terminates.

## Measuring useful concurrency

`std.time.measure action` returns `(elapsedNanoseconds, result)` using a
monotonic clock. To compare sequential and concurrent versions:

1. Run the same inputs and the same operation.
2. Keep correctness checks identical.
3. Warm up if startup dominates.
4. Repeat enough times to see variance.
5. Report both latency and resource bounds.

For network programs, use a controlled local service with deliberate response
delays before drawing conclusions from the public internet. Remote caches,
rate limits, and network variation can otherwise overwhelm the effect you want
to measure.

Next: [Networking and HTTP](07-networking-and-http.md).
