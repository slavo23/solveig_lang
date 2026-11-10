# The Solveig tutorial

This is a course in building reliable Solveig programs, not a catalog of
syntax. It follows two applications that are compiled and exercised in this
repository:

- [Log Audit](../../examples/log_audit/) turns several log files into one
  deterministic report. It begins as pure text analysis and grows into a
  bounded, concurrent file program.
- [Release Radar](../../examples/release_radar/) checks release metadata over
  HTTP. Its default mode is a hermetic loopback demonstration; live GitHub
  access is opt-in.

The intended reader is a working programmer or CS student. Familiarity with
functions and ordinary data structures helps, but neither Haskell nor prior
functional-programming experience is assumed. Experienced FP readers can use
the **Skip ahead** notes in each chapter.

## How the course works

Every chapter has the same contract:

1. start from an application need;
2. build the language model needed to meet it;
3. point to the exact checked source that uses the idea;
4. show a deliberate mistake and explain the compiler diagnostic;
5. finish with a runnable checkpoint, exercises, and a recap.

Complete source links always name repository files. Short snippets are copied
from those sources or are explicitly labeled as isolated illustrations. Claims
about evaluation, typing, cancellation, ownership, buffering, or limits are
grounded in the compiler, VM, standard-library source, and executable tests.

Run either application from the repository root with Cargo:

```sh
cargo run -- --project-root examples/log_audit
cargo run -- --project-root examples/release_radar
```

With an installed compiler, omit `cargo run --`:

```sh
solveig --project-root examples/log_audit
```

## Core course

1. [Run, change, and diagnose a program](01-getting-started.md) establishes the
   package model and the read–compile–run loop with Log Audit.
2. [Transform log data with pure functions](02-language-basics.md) introduces
   values, expressions, functions, lists, recursion, local bindings, tuples,
   and readable operator use.
3. [Model valid states with data and patterns](03-data-and-patterns.md) turns
   file outcomes and summaries into algebraic data rather than conventions.
4. [Cross the IO boundary](05-io-and-files.md) covers `IO`, `Task`, paths,
   filesystem errors, cleanup, streams, environment, time, randomness, and
   child processes.
5. [Organize and test an application](04-abstraction.md) covers polymorphism,
   higher-order functions, modules, visibility, typeclasses, collections, and
   deterministic checks.
6. [Decode an external service response](08-parsers-and-json.md) moves into
   Release Radar through parser replies, bounded JSON, and domain decoding.
7. [Build a typed HTTP client](07-networking-and-http.md) develops requests,
   responses, limits, errors, authentication, redirects, and a local fixture.
8. [Make independent work overlap](06-concurrency.md) explains structured
   concurrency, bounded mapping, channels, backpressure, cancellation, and why
   the two applications choose different concurrency tools.
9. [Finish and operate the application](09-packages-and-tooling.md) covers
   configuration, caching, CLI arguments, packages, tests, formatting, and the
   editor.

The file numbers remain stable for existing links; the sequence above is the
pedagogical order.

## Deep workshops

After the course, use the [workshop index](workshops/README.md):

- build and reason about Log Audit's manual bounded worker pool;
- trace Scry from source text through parsing and evaluation;
- implement and test message framing over TCP and UDP;
- stream files, HTTP bodies, and child-process pipes without hidden growth;
- understand the ray tracer from vector algebra to bounded PPM output.

## Reading conventions

Code marked `solveig` belongs in an `.sv` file. Shell commands are marked
`sh`, manifests are `toml`, and expected terminal output is `text`.

Signatures are included when they clarify a boundary. The examples use `(.)`
for genuine transformation pipelines and `($)` to remove parenthesis noise;
they retain named arguments when point-free style would obscure control flow,
resource lifetime, or error policy.

The tutorial teaches choices. The [documentation map](../README.md) leads to
the exhaustive reference.

Continue with [Run, change, and diagnose a program](01-getting-started.md).
