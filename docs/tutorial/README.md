# The Solveig tutorial

Welcome to Solveig. This tutorial starts with small pure functions and grows
into programs that read files, recover from typed errors, coordinate concurrent
work, communicate through channels, and make HTTP requests.

You do not need prior functional-programming experience. If you know another
language, the main adjustment is that Solveig keeps pure computation separate
from interaction with the outside world. That separation gives the compiler
useful information without making practical programs awkward.

## Learning path

Read the chapters in order on a first pass:

1. [Getting started](01-getting-started.md) creates and runs a package.
2. [Language basics](02-language-basics.md) covers values, functions,
   expressions, lists, tuples, and operators.
3. [Data, patterns, and errors](03-data-and-patterns.md) introduces algebraic
   data types, exhaustive pattern matching, `Maybe`, `Result`, and typed IO.
4. [Abstraction](04-abstraction.md) covers modules, polymorphism, type aliases,
   typeclasses, instances, higher-order functions, and point-free style.
5. [IO and files](05-io-and-files.md) explains `Task`, `do`, handles, cleanup,
   filesystem operations, environment variables, and processes.
6. [Concurrency](06-concurrency.md) develops a precise mental model for green
   threads, structured lifetimes, bounded parallelism, channels, cancellation,
   and timeouts.
7. [Networking and HTTP](07-networking-and-http.md) covers TCP and the
   high-level HTTP client, including concurrent requests and a fully local,
   testable web program.
8. [Parsers and JSON](08-parsers-and-json.md) shows generic parser combinators,
   incremental replies, JSON decoding, and input limits.
9. [Packages and tooling](09-packages-and-tooling.md) explains local libraries,
   the CLI, program arguments, and the editor.

Reference pages under `docs/` are organized by API. The tutorial is organized
by what you are trying to learn.

## Runnable tutorial projects

Complete projects live in [`code/`](code/), not in the repository's example
applications:

- [`language_tour`](code/language_tour/) exercises data, patterns, aliases,
  typeclasses, higher-rank functions, tuples, and point-free constructors.
- [`file_summary`](code/file_summary/) reads a text file and writes a report.
- [`concurrent_jobs`](code/concurrent_jobs/) runs a bounded worker pool.
- [`local_http`](code/local_http/) starts a loopback TCP server and fetches from
  it with three concurrent HTTP clients.

They are executed by the repository test suite, so the checked-in programs
cannot silently drift away from the language and standard-library APIs. Run one
from the repository root with:

```sh
cargo run -- --project-root docs/tutorial/code/concurrent_jobs
```

With an installed compiler, use:

```sh
solveig --project-root docs/tutorial/code/concurrent_jobs
```

The file project creates `summary.txt`. If you are experimenting, copy that
project to a temporary directory first or remove the generated file afterward.

## Conventions

Code marked `solveig` belongs in an `.sv` source file. Shell commands are
marked `sh`; manifest files are marked `toml`.

Most examples use explicit signatures. Solveig can infer many of them, but a
signature documents intent and makes error messages easier to understand.
Standard-library imports are explicit; the Prelude is automatically in scope.

Continue with [Getting started](01-getting-started.md).
