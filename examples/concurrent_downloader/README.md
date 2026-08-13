# Concurrent downloader

By default, this example resolves `neverssl.com` and downloads four plain-HTTP
resources through a two-worker bounded-channel pool. It demonstrates real DNS,
internet TCP readiness, bounded-channel backpressure, response processing,
progress reporting, typed failure recovery, and concurrent file output.

The four responses replace `downloads/download-1.txt` through
`downloads/download-4.txt` on every run. The directory ignores generated files
through its own `.gitignore`, and each HTTP response is capped at one megabyte.
Consequently, rerunning the demo cannot create additional files or grow its
managed output beyond four megabytes.

Internet requests have a five-second real-time timeout. The offline mode omits
that wrapper so deterministic virtual time cannot race its loopback server.

Run it from the repository root with:

```sh
cargo run -- --project-root examples/concurrent_downloader
```

This is deliberately a minimal HTTP/1.0 demo. It downloads public, untrusted
content over unencrypted HTTP and does not implement TLS, redirects, chunked
transfer decoding, or a production HTTP client.

For a deterministic offline run, pass `--local` to start the original
in-program loopback server. The golden suite uses this mode through `args.txt`:

```sh
cargo run -- --project-root examples/concurrent_downloader -- --local
```

The local mode downloads four resources with the same two workers. Three
succeed, while one returns an HTTP error that is handled with `catchError`.
