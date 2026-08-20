# Release Radar

Release Radar is a compact end-to-end example of practical Solveig I/O:

- forty repository requests run through `std.task.mapConcurrentlyBounded` with
  a bounded four-worker pool;
- the default local mode executes the same delayed workload sequentially and
  concurrently, reports both monotonic timings, and fails if concurrency is not
  faster;
- the default mode starts a deterministic local GitHub-like service;
- `--live` uses verified HTTPS against the GitHub Releases API;
- every response has explicit timeout, redirect, header, and body bounds;
- JSON is parsed through `std.json`, and results print in configuration order;
- ETags are retained in one overwritten `.release-radar-cache` file, so cache
  storage cannot grow on every run.

Run the reproducible local demonstration:

```console
cargo run -- --project-root examples/release_radar
```

Run real concurrent HTTPS requests (subject to network availability and GitHub
rate limits):

```console
cargo run -- --project-root examples/release_radar -- --live
```

Edit `repos.txt` to select any non-empty set of `owner/repository` values. The
program treats each line as opaque configuration: repository identities are not
compiled into its source. The library combinator indexes work before dispatch
and restores configuration order after concurrent completion.

## Project structure

- `main.sv` owns CLI mode selection, measurement, and reporting.
- `release_radar/client.sv` owns HTTP request construction and bounded fetching.
- `release_radar/model.sv` owns release decoding and presentation.
- `release_radar/cache.sv` owns the persistent-map ETag cache and fixed-file
  serialization.
- `release_radar/fixture.sv` owns the delayed loopback service used by the
  interactive performance demonstration.

This application is deliberately not registered as a golden or integration
test. The automated HTTP suite uses its own hermetic protocol fixture and never
contacts the internet; Release Radar's `--live` network access occurs only when
you request it explicitly.
