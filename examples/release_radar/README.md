# Release Radar

Release Radar is a compact end-to-end example of practical Solveig I/O:

- forty repository requests run through `std.task.mapConcurrentlyBounded` with
  a bounded four-worker pool;
- each worker prints start and completion events immediately, while the final
  report remains in deterministic configuration order;
- the default local mode executes the same variably delayed workload
  sequentially and concurrently, reports both monotonic timings, and fails if
  concurrency is not faster;
- `--live` loads optional authentication, checks GitHub's current allowance,
  then uses verified HTTPS against the Releases API;
- every response has explicit timeout, redirect, header, and body bounds;
- primary rate exhaustion, secondary throttling, invalid credentials, missing
  releases, transport failures, and malformed JSON have distinct messages;
- ETags are merged into one overwritten `.release-radar-cache` file, preserving
  older entries for repositories that fail and preventing per-run growth.

Run the reproducible local demonstration:

```console
cargo run -- --project-root examples/release_radar
```

## GitHub authentication

GitHub rate-limits API access. For authenticated requests, copy the value-free
template and put a read-only personal access token in the ignored file:

```console
cp examples/release_radar/.env.example examples/release_radar/.env
```

```text
GITHUB_TOKEN=your-token
```

The process environment takes precedence, so an exported `GITHUB_TOKEN` wins
over `.env`. The strict loader reads at most 64 KiB, performs no interpolation
or shell execution, and never prints the token. Live requests disable redirects
so credentials cannot be forwarded to another origin.

Run real concurrent HTTPS requests:

```console
cargo run -- --project-root examples/release_radar -- --live
```

Edit `repos.txt` to select any non-empty set of `owner/repository` values. The
program treats each line as opaque configuration: repository identities are not
compiled into its source. Blank lines and lines beginning with `#` are ignored.

`[done ...]` lines are printed by workers as requests actually finish and will
usually differ from `repos.txt` order. The library combinator separately indexes
work before dispatch and restores configuration order for the final report.
Stable final output is therefore a useful ordered-map guarantee, not evidence
of sequential execution.

## Project structure

- `main.sv` owns CLI mode selection, dotenv precedence, measurement, and
  reporting.
- `release_radar/client.sv` owns GitHub headers, access checks, bounded fetching,
  and completion events.
- `release_radar/model.sv` owns typed GitHub outcomes, JSON decoding, and
  presentation.
- `release_radar/cache.sv` owns merged persistent ETags and fixed-file
  serialization.
- `release_radar/fixture.sv` owns the delayed loopback service used by the
  interactive performance demonstration.

The default mode is local. Release Radar contacts GitHub and reads live
credentials only when you request `--live` explicitly.
