# Workshop: a bounded worker pool

This workshop studies the complete
[`examples/log_audit`](../../../examples/log_audit/) application. Run the
baseline first:

```sh
cargo run -- --project-root examples/log_audit
```

## State the protocol before reading code

There are five `LogJob` values, three workers, a capacity-two jobs channel, a
capacity-two results channel, and one collector. The producer owns closure of
the jobs channel. The collector expects exactly five outcomes. `main` closes
the results channel only after collection.

```text
producer ──send──▶ jobs(2) ──recv──▶ three scoped workers
                                             │
                                         read file
                                             │
collector ◀──recv── results(2) ◀──send────────┘
```

These facts are not all expressible in `Chan a`. Types guarantee that jobs and
results cannot be mixed; the program structure must guarantee who closes and
how loops terminate.

## Separate worker policy from worker lifetime

`auditFile` reads and analyzes one path. `auditOne` decides that a failed
`readFile` operation becomes `MissingLog` and rethrows other failures.
`workerStep` repeats receive–audit–send. `worker` treats
`ResourceVanished` from the closed jobs channel as normal termination.

Read these four definitions in
[`main.sv`](../../../examples/log_audit/src/main.sv). Their separation answers
four different questions:

1. What is one unit of domain work?
2. Which failures are expected domain outcomes?
3. How is another job requested?
4. How does the long-lived worker stop?

Do not put closure of the shared result channel inside a worker. No individual
worker knows that every peer has finished.

## Why producer and collector overlap

`runAudit` scopes three workers, then runs:

```solveig
concurrently
    (enqueueJobs jobs)
    (collectResults results 5 emptySummary)
```

With bounded channels, this overlap is required for liveness. If all enqueuing
ran first, workers could fill the result queue, block, stop consuming jobs, and
leave the producer blocked on the full jobs queue. Starting the collector
breaks that wait cycle.

Draw a wait-for graph whenever a bounded pipeline can stall. A cycle means no
capacity increase is a principled repair; ownership or scheduling must change.

## Prove the bounds

At any moment:

- at most two jobs wait in the jobs channel;
- at most three jobs are being processed;
- at most two results wait in the results channel;
- the collector retains one fixed-size `AuditSummary`;
- each file is read strictly, so peak input memory still includes the largest
  concurrently read files.

Channel bounds do not bound file size. To audit untrusted or large files,
replace strict `readFile` with a limited read or a scoped line/byte stream and
fold directly into `LogCounts`.

## Determinism without ordered results

Workers may finish in any order. `includeResult` combines counts through
addition, so the final summary is invariant under result permutation. This is
stronger and cheaper than tagging every job with an index merely to restore
order.

Ordering would matter if the report listed one line per input path. In that
version, send `(index, result)` and reconstruct input order, or use
`mapConcurrentlyBoundedOrdered`.

## Failure and cleanup trace

Trace a permission error:

1. `readFile` returns `FsError`;
2. `mapError fsToIO` converts it at the application boundary;
3. `auditOne` sees an operation it does not classify as missing and calls
   `failIO`;
4. the worker exits with a typed error;
5. explicit `withThread` ownership still interrupts and joins children while
   scopes unwind.

The raw child error does not automatically become `runAudit`'s result because
`withThread` is a lifetime primitive, not a value-joining primitive. A
production redesign that must surface per-worker failure should send a typed
outcome or use a structured result combinator.

## Experiments

Work in a copy:

1. Set both channel capacities to one and confirm progress.
2. Increase the job count but forget the collector count. Observe whether the
   protocol blocks or returns early, then repair the single source of truth.
3. Add a per-file result that includes `Path`; decide whether output order is a
   contract.
4. Stream a large synthetic log and make peak memory depend on chunk size
   rather than file size.

## Review checklist

- Is every queue bounded or proven finite?
- Who closes each channel?
- What wakes every blocked participant on cancellation?
- Does every produced job yield exactly one collected outcome?
- Is result order meaningful or algebraically irrelevant?
- Are worker failures represented, propagated, or intentionally normalized?
