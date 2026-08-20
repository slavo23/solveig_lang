# Release Radar

Release Radar is a compact end-to-end example of practical Solveig I/O:

- forty repository requests run through a bounded four-worker pool;
- the local service delays each response, so its concurrent server and client
  pools are exercised rather than merely issuing fast sequential-looking calls;
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
compiled into its source. Results are indexed before dispatch and restored to
configuration order after concurrent completion.
