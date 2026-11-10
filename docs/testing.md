# Testing Solveig

Solveig separates quick compiler feedback from comprehensive application and
platform validation. The separation is about scheduling, not importance: the
complete gate remains mandatory before a change is finished.

## The fast path

Run this while changing the compiler, runtime, formatter, or libraries:

```sh
cargo test
```

This runs the fast compiler conformance target owned by the root package. The
runner uses small test-owned programs from `tests/golden/`, checks successful
execution and failures from the parser through the type checker, and includes a
verified CLI smoke test. When changing an individual crate, also run its focused
unit tests with `cargo test -p <crate>`; the complete gate below runs every
crate's units automatically.

Conformance cases share one explicit `Compilation` only to reuse immutable
Prelude and standard-library artifacts. Each case replaces the complete local
entry source, receives fresh checked local artifacts and a fresh VM, and
reports independently. A separate regression compares a reused compilation
with a cold compilation so cache reuse cannot silently change semantics.

The equivalent repository command is:

```sh
cargo xtask test fast
```

## Focused comprehensive tiers

Use the smallest tier that owns the behavior you changed:

```sh
cargo xtask test host
cargo xtask test ide
cargo xtask test examples
cargo xtask test stress
```

- `host` covers runtime goldens, files, processes, HTTP, TLS, and formatter CLI
  behavior that crosses an operating-system boundary.
- `ide` covers retained compilations, overlays, editor queries, and the full
  LSP protocol-adapter suite.
- `examples` executes the user-facing applications, including the deliberate
  512-by-512 ray-tracer capacity regression.
- `stress` runs VM GC stress and the explicitly ignored 1,000-edit LSP
  endurance scenario.

All required non-stress coverage runs with:

```sh
cargo xtask test all
```

This command enables every test feature in one Cargo invocation. It runs the
fast workspace suite, the complete 160-test integration suite, and the full LSP
suite without compiling three feature graphs sequentially. It is the
pre-completion gate and the command CI should invoke.

Stress remains conditional: run it when a change affects the VM, heap, retained
IDE state, or another boundary named by the planning records. It deliberately
trades feedback time for repeated-state and collector pressure.

## Where a new test belongs

Prefer the lowest layer that can prove the property:

1. A pure algorithm or representation invariant belongs beside its owning
   crate as a unit test.
2. A language rule belongs in the compiler-owned conformance or diagnostic
   corpus as the smallest source that distinguishes correct behavior.
3. One tiny test-owned package proves the complete CLI pipeline. Do not use an
   example application as the only coverage for a language rule.
4. Filesystem, process, socket, TLS, and other host contracts belong in the
   hermetic host tier. Use temporary directories and loopback services, never
   the public network.
5. Editor derivation belongs in the IDE tier; protocol conversion belongs in
   the LSP crate.
6. Examples prove that the documented application still works, but may evolve
   without redefining compiler semantics.
7. Large capacities, 1,000-edit workloads, fuzzing, and GC pressure belong in
   stress or scheduled CI.

Tests must use fixed seeds, virtual time where the scheduler provides it,
loopback networking, exact outputs or structured diagnostics, and no sleeps as
synchronization. A slow test should expose its reason in its tier rather than
making every edit pay for an application-scale workload.
