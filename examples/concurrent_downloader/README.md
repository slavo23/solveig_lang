# Concurrent downloader

This example runs a small HTTP/1.0 server on an OS-assigned loopback port and
downloads four resources through a two-worker bounded-channel pool. Three
resources succeed, while one returns an HTTP error that the worker converts to
a typed `IOError` and handles with `catchError`.

The server lives inside the Solveig program, so the example and its golden test
are hermetic: they use no internet service, real-time delay, TLS, redirect, or
external process. The point is to exercise TCP readiness, structured threads,
bounded-channel backpressure, response processing, progress reporting, and
typed failure recovery together.

Run it from the repository root with:

```sh
cargo run -- --project-root examples/concurrent_downloader
```
