# Strict streams

`std.stream` is Solveig's one protocol for incremental values. A
`Stream scope e a` is strict, pull-based, and effect-aware: each call to
`next` performs only enough work to report completion or yield one `a` and the
remaining stream.

Use a finite `List` when all values are already in memory. Use a `Stream` when
values arrive incrementally, may be unbounded, or belong to an open resource
such as a file. Streams are not lazy lists: effects happen inside `IO e`, at
the pull that requests them.

## Pull protocol

```solveig
import std.stream(Stream, StreamStep(..), next)

first : forall scope e a. Stream scope e a -> IO e (Maybe a)
first stream = do
    step <- next stream
    case step of
        StreamDone => pureIO Nothing
        | StreamYield value remaining => pureIO (Just value)
```

`StreamYield` owns the continuation of the sequence. Always continue with its
`remaining` stream. In particular, do not call `next` repeatedly or
concurrently on the same resource-backed value: that would repeat or race the
same underlying pull. The common combinators follow this single-consumer rule
for you.

A pull does not prefetch the following value. `map` and `take` buffer at most
one value; `filter` pulls until it finds one matching value; `chunksOf size`
buffers at most `size` values; `toListLimited limit` retains at most `limit`
values and pulls one additional value only to establish overflow. All preserve
input order.

## Finite sources and transformations

```solveig
import std.stream

sumEvenPrefix : Task Int
sumEvenPrefix =
    std.stream.fold
        (fun total value => total + value)
        0
        (std.stream.take
            10
            (std.stream.filter even
                (std.stream.map
                    (fun value => value * 2)
                    (std.stream.fromList [1, 2, 3, 4, 5, 6]))))
```

`fromList` streams an existing list without copying it. `fromText` yields
Unicode scalar values directly from packed UTF-8 text, and `fromBytes` yields
integer byte values directly from packed bytes. The pure constructors use the
neutral scope `()` because they own no external resource.

`fold` consumes in constant collection space. `forEach` performs one action at
a time in stream order. `toListLimited` is the deliberate boundary back to a
finite list:

```solveig
small : Task (Either std.stream.StreamError [Int])
small = std.stream.toListLimited 100 (std.stream.fromList [1, 2, 3])
```

A negative collection limit and a non-positive chunk size are explicit
`StreamError` values and do not pull upstream. Collection overflow also returns
`StreamError`; it is not confused with the stream's own typed IO error `e`.
Use `mapError` to transform that typed pull error.

## Scoped files and early release

`std.io.withFileByteStream` is the first resource producer using this protocol:

```solveig
import std.bytes(bytesLength)
import std.io(withFileByteStream)
import std.stream(Stream, fold, take)

countFirstChunks : forall scope. Stream scope HostError Bytes -> Task Int
countFirstChunks stream =
    fold
        (fun total bytes => total + bytesLength bytes)
        0
        (take 3 stream)

main : Task ()
main = do
    byteCount <- withFileByteStream 4096 "events.bin" countFirstChunks
    printLine ("read " <> show byteCount <> " bytes")
```

The first argument is the maximum bytes in each pull. The function opens the
file, invokes a callback that works for every fresh `scope`, then closes the
file when that callback finishes. Returning after `take 3` therefore releases
the file without reading the rest. Cleanup also runs after a typed failure, a
defect, or interruption.

The higher-rank callback is important. Its result cannot contain the fresh
scope, so neither the stream nor a transformed stream derived from it can
escape the file lifetime. This is a compile-time ownership boundary rather
than a garbage-collector finalizer.

The positive chunk size is a buffering choice, not a total file-size limit.
Ordinary `readFile` remains available when reading a complete trusted file is
the simpler operation; use the stream when memory must stay bounded or the
consumer may stop early.

## Failure and cancellation

A stream does not catch failures. Its `next`, `fold`, and `forEach` actions
propagate typed `IO e` failures, defects, and interruption immediately. A
resource owner such as `withFileByteStream` brackets the complete callback, so
the same propagation path releases the resource before control reaches an
outer `catchError`, `catchDefect`, or interrupted thread's owner.

There is no producer thread or hidden queue in `std.stream`. Pulling establishes
backpressure directly: a slow consumer simply does not request the next value.
Concurrency remains explicit through `std.task`, `std.thread`, and
`std.channel`; a stream combinator does not silently reorder or parallelize
effects.
