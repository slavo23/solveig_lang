# Concurrent log audit

This application is a practical bounded worker pool. Three scoped green
threads read five requested log files. One path is deliberately absent; its
`ReadFileError` is handled locally with `catchError` and becomes a normal
`MissingLog` result instead of aborting the batch.

Both channels have capacity two. The small job channel prevents the producer
from running arbitrarily far ahead, and the small result channel prevents
workers from accumulating unconsumed analyses. File reads use the VM's bounded
blocking pool, so a worker waiting on disk never blocks the Solveig scheduler.

Results may arrive in any host-completion order. The collector therefore uses
an order-independent summary and emits output only after receiving exactly one
result per request. `withThread` scopes all workers; `concurrently` runs the
producer and collector together so bounded backpressure cannot deadlock them.

The final report always replaces `output/report.txt`, whose directory ignores
that generated file. Repeated runs neither create additional files nor grow
the output without bound. The program reads the report back and prints it,
making the actual written contents visible after every run.

Run it from the repository root:

```sh
cargo run -- --project-root examples/log_audit
```

Expected report:

```text
Solveig log audit
files analyzed: 4
files missing: 1
events: 14
info: 8
warnings: 3
errors: 3
```
