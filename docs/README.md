# Solveig documentation

The documentation has four deliberately different jobs. Choose the route that
matches what you are doing instead of reading the repository as one long book.

## Learn the language

The [tutorial](tutorial/README.md) is the first-reading path. It follows two
real applications:

1. **Log Audit** teaches expressions, functions, algebraic data, pattern
   matching, typed failures, files, modules, tests, channels, and scoped
   workers.
2. **Release Radar** teaches JSON decoding, HTTP client design, authentication,
   bounded concurrency, caching, configuration, and operational failure.

The [workshops](tutorial/workshops/README.md) go deeper after the course. Each
one studies an executable repository application or fixture and explains why
its design is safe, bounded, and testable.

## Solve a practical problem

- [IO, handles, files, paths, environment, time, randomness, and processes](io.md)
- [Strict streaming and bounded data flow](streaming.md)
- [Structured concurrency, cancellation, channels, and timeouts](concurrency.md)
- [HTTP and HTTPS clients](http.md)
- [Packages, modules, imports, visibility, and dependencies](packages.md)
- [Formatting](formatting.md)
- [VS Code and the language server](lsp.md)
- [Reading compiler diagnostics](diagnostics.md)

These guides explain policy and common compositions. They link to the exact
modules that own individual APIs.

## Look up an API

- [Prelude](prelude.md)
- [Standard library](standard-library.md)
- [Errors and defects](errors.md)

The library source under `lib/prelude/src` and `lib/std/src` is authoritative.
The reference pages summarize that source; they do not define a second API.

## Understand or change the implementation

- [Virtual machine](vm.md)
- [Debugging hooks](debugging-hooks.md)
- [Benchmark policy and recorded methodology](benchmarks.md)
- [`plan_v3/`](../plan_v3/) for architecture, decisions, registries, and
  evidence-backed progress

Implementation notes assume familiarity with the tutorial and are not part of
the first-reading path.
