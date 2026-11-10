# Workshop: strict streams and child processes

Solveig is strict. A streaming API therefore must represent demand and resource
lifetime explicitly; laziness is not inferred from a list type. This workshop
uses [the streaming guide](../../streaming.md) and the executable
[`process_capture`](../../../tests/golden/run/process_capture/src/main.sv)
fixture.

Run the fixture through:

```sh
cargo test --test golden_runner
```

The golden runner supplies the current Solveig executable as the child program,
so the test covers real argument, environment, stdin, stdout, stderr, timeout,
termination, and reaping behavior without a shell.

## Read a stream as a protocol

`Stream scope e a` is a demand-driven sequence whose next pull may perform
`IO e`. A pull returns either completion or one value plus the remaining
stream. Combinators such as `fold`, `forEach`, `take`, and `mapStream`
thread that remainder correctly.

The `scope` identifies the owning file, response, socket, or process callback.
It prevents the stream or any transformed stream derived from it from escaping
after the owner closes. It does not itself impose a total byte limit.

Streams are single-consumer. Pulling the same stream value concurrently would
duplicate a logical position, so continue only with the returned remainder or a
library combinator.

## Distinguish three bounds

- A **chunk bound** limits one host read or write.
- A **total quota** limits cumulative data accepted by the operation.
- A **retention bound** describes how much the consumer keeps.

A 64 KiB file chunk folded with `bytesAppend` into one result still retains the
whole file. A strict 1 MiB HTTP response is totally bounded even if iteration
later exposes smaller body slices. State all three policies at a trust boundary.

## Direct child execution

`process executable arguments` validates a direct invocation. There is no
shell, glob expansion, interpolation, pipeline syntax, or quoting layer.
Builders configure working directory, environment changes, stdin, and
stdout/stderr capture or discard policy.

Captured output is `Bytes`. `processStdoutText` and
`processStderrText` perform explicit UTF-8 validation. A nonzero exit status is
a successful process execution result, not a transport error; the application
decides which status values are acceptable.

`runProcessLimited request n` applies `n` independently to each captured
output stream. Use `runProcess` only for trusted, small output.

## Drain both pipes concurrently

The fixture's strongest process test makes the child write 163,840 bytes to
both stdout and stderr:

```solveig
withProcess
    4096
    request
    (fun child =>
        concurrently
            (streamByteCount (processStdout child))
            (streamByteCount (processStderr child)))
```

Sequential draining can deadlock. If the parent waits for stdout EOF while the
child blocks on a full stderr pipe, neither can advance. Concurrent folds keep
both bounded OS pipes moving. Discarding one stream at process configuration
time removes the need to drain that stream.

## Stream stdin under the same chunk policy

`withProcess 16 (pipeStandardInput request) callback` limits one stdin write
to 16 bytes. The fixture proves a 17-byte write fails, then sends
`"bounded-"` and `"stdin"` as two valid writes, closes stdin twice
idempotently, waits, and checks the echoed byte count.

An interactive protocol must close stdin when no more messages remain; many
children wait for EOF before exiting. The caller must also keep draining output
if the child can respond before stdin is complete.

## Termination, timeout, and reaping

`terminateProcess` requests explicit early shutdown and returns the reaped
exit status. Leaving `withProcess` while the child is running closes pipes,
terminates the direct child, and waits for it unconditionally.

Wrapping the scoped session in `timeout` interrupts it. The scope cleanup still
reaps the child before returning `Nothing`; timeout is not permission to leak a
zombie or background child.

The guarantee is for the direct child. A subprocess that deliberately detaches
descendants creates a different operating-system ownership problem.

## Compose file, HTTP, and process streams

A repository inspection tool might:

1. stream paths from a large file;
2. start a bounded number of direct child commands;
3. stream each child's stdout and stderr;
4. decode complete records with a framing limit;
5. send summaries through a bounded channel;
6. stream a final upload with `withStreamResponse`.

Every arrow needs an owner and bounds. Do not place a scoped stream in a channel:
its `scope` should fail to escape. Send owned chunks or decoded domain values
instead.

## Experiments

1. Change the process chunk size and prove total byte counts stay the same.
2. Remove concurrent stderr draining in a scratch fixture and explain the
   possible pipe cycle without relying on a hang as a test.
3. Add a total quota to `streamByteCount`.
4. Stream lines from a file into a child protocol, keeping writes within the
   process chunk bound.
5. Cancel a blocked child session and confirm the following test can still
   spawn and wait for a child.

## Review checklist

- What owns the stream and can it escape?
- What are chunk, total, and retention bounds?
- Are stdout and stderr drained concurrently?
- When and by whom is stdin closed?
- How are nonzero exit, typed IO failure, timeout, and defect distinguished?
- Is every direct child reaped on every exit path?
