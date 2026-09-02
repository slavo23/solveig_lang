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

## Parsing scoped chunks

`std.parser.parseStream` consumes the existing stream protocol rather than
introducing a parser-specific effectful source. Its stream elements are token
chunks, so a UTF-8 text stream can be adapted with one ordinary `map`:

```solveig
import std.parser
import std.stream
import std.text

parseTextChunks : forall scope e custom value.
    std.parser.Parser (std.parser.StreamInput Char) custom value ->
    std.stream.Stream scope e String ->
    IO e (Either (std.parser.ParseError custom) value)
parseTextChunks parser chunks =
    std.parser.parseStream
        show
        parser
        (std.stream.map std.text.stringToChars chunks)
```

The parser requests another chunk only after returning `Awaiting`; the bridge
does not read ahead. Stream failures remain typed `IO e` failures. Grammar
rejections are returned in the inner `Either`, retaining their structured
position and custom error values. This separation lets the action run directly
inside a rank-2 resource callback without returning a stream or cursor.
The current `StreamInput` retains **all supplied tokens** until parsing ends;
incremental pulling does not promise bounded parser memory. Narrow `attempt`
and well-placed `cut` reduce backtracking work but do not release that storage.

## Scoped child pipes

`std.process.withProcess` applies the same scope rule to a live direct child.
`processStdout` and `processStderr` perform one bounded host read per pull and
yield non-empty `Bytes`; no following chunk is prefetched. If both captured
streams may exceed an operating-system pipe buffer, drain them as two separate
streams with `std.task.concurrently`. Pulling stdout to EOF before starting
stderr is not generally safe, because a full stderr pipe can stop the child
before it closes stdout.
Obtain each process output stream once and preserve the general stream
single-consumer rule. Stdin writes on one handle are likewise sequential.

The request chooses capture or discard independently. A discarded stream is
exposed as empty while its host pipe drains eagerly, so ignoring it cannot
deadlock the child. `pipeStandardInput` enables explicit writes. Each write is
bounded by the same positive chunk size and completes only after the whole
chunk is written and flushed; `closeProcessStdin` then delivers EOF.

Returning from the callback closes all pipe workers and terminates and reaps a
still-running direct child. `waitProcess` does not consume output, so a program
may wait and then finish draining finite captured streams. A timeout or thread
interruption follows the same cleanup path. Neither the handle nor either
stream can escape the callback's fresh scope.

## Scoped HTTP responses

`std.http.withResponse` exposes status, headers, and a scoped `Bytes` stream.
`withResponseWith` additionally accepts immutable `HttpOptions`; `withChunkBytes`
chooses a positive per-pull bound and `withMaximumResponseBytes` optionally
sets a cumulative transfer quota.
The callback is entered after headers arrive; each subsequent pull reads at
most one chunk. The host protocol buffer is bounded, but it may already hold
some body bytes received with the headers. There is no whole-response buffer
unless the consumer builds one.

`requestStream` applies the same pull protocol to an upload. The synchronous
host transport requests one chunk, then waits for that exact Solveig pull to
complete; a one-slot rendezvous prevents read-ahead. `withStreamResponse`
transfers the same scoped resource from upload ownership to response-body
ownership after headers arrive. Stream request bodies are one-shot and cannot
be replayed across a 307 or 308 redirect.

Returning early closes without draining or validating an unread response
suffix. Cleanup also runs after upload-source failure, response failure,
defect, and interruption, and joins the host owner thread. An already-blocked
host operation must finish or reach the original HTTP deadline before queued
close completes; this is timeout-bounded cleanup, not immediate socket
cancellation. See
[HTTP and HTTPS](http.md) for the complete policy and a counting example.

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
