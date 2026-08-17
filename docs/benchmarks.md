# Benchmarks

Solveig's performance work is evidence-driven. The benchmark suite separates
small VM kernels from representative source programs, validates every result
before timing it, and keeps capacity tests out of the routine edit loop.

## Running benchmarks

Build and run the complete statistical suite with:

```bash
cargo bench --bench vm --bench language
```

During optimization work, run only the affected benchmark. For example, the
dictionary-specialization task starts with:

```bash
cargo bench --bench language -- language/arithmetic
```

Use `-- --list` to compile a harness, validate all of its expected results, and
list its cases without collecting samples:

```bash
cargo bench --bench language -- --list
cargo bench --bench vm -- --list
```

The complete suite is run when establishing a baseline, when an optimization
could affect several categories, and at the Phase O exit gate. It is not part
of the normal edit-test loop.

Criterion stores detailed reports under `target/criterion/`. That directory is
build output and is not committed.

## What is measured

The `vm` harness uses small, deterministic bytecode programs. Bytecode
construction is outside the timed sample. Each sample still includes cloning
the compiled program, VM initialization, execution, garbage collection, and
result teardown; it is not presented as instruction dispatch in isolation.

| Benchmark | Work per sample |
|---|---|
| `vm/arithmetic_dispatch` | 250,000 integer loop iterations over loads, equality, addition, subtraction, branches, and backward-jump safe points |
| `vm/allocation_gc` | Builds a rooted 100,000-object linked structure, crossing nursery collections |

The `language` harness runs a real package through `solveig-driver`. One
`Compilation` is retained per case, so package-check caching is warm after the
first iteration, while the public check-to-run path, lowering, bytecode
emission, VM startup, and execution remain in every sample. These measurements
therefore show user-visible warm compiler/runtime performance rather than a
synthetic interpreter-only number.

| Benchmark | Work per sample |
|---|---|
| `language/allocation` | Builds 50,000 list cells and tail-recursively sums them |
| `language/arithmetic` | Runs 200,000 concrete `Int` iterations using typeclass operators and `mod`; this is the primary O2 signal |
| `language/closures` | Applies one capturing closure through a higher-order function 200,000 times |
| `language/patterns` | Traverses a shared algebraic tree eight times, performing 524,288 leaf matches plus branch matches |
| `language/string_bytes` | Round-trips text through characters, then performs 20,000 byte slices, appends, indexes, and length queries |
| `language/file_io` | Reads the 3,588-byte benchmark source 64 times through the IO worker path and validates non-empty results |
| `language/channels` | Sends and receives 10,000 integers through a capacity-64 channel between two green threads |

The file workload deliberately uses a fixed, read-only input. It neither leaves
outputs behind nor grows disk usage between samples. Since repeated reads are
normally served by the operating-system cache, this case measures Solveig's
handle/worker/runtime overhead on small reads, not storage-device bandwidth.

## Baseline: 2026-08-17

The baseline was collected from parent revision `b05e483` plus the O1 benchmark
harness, before any Phase O optimization.

- Machine: MacBook Air, Apple M2, 8 cores (4 performance and 4 efficiency),
  16 GB memory
- OS: macOS 26.3.1 (25D2128), arm64
- Toolchain: rustc/cargo 1.96.1, LLVM 22.1.2
- Harness: Criterion 0.7.0, optimized Cargo bench profile, Plotters backend
- Sampling: 1 second warm-up, 20 samples, 3 second requested measurement; the
  harness allowed Criterion to extend collection when a case needed more time
- Correctness: each case ran once and compared its complete deterministic
  output before sampling

Criterion reports the lower bound, point estimate, and upper bound of its
confidence interval:

| Benchmark | Baseline time |
|---|---:|
| `vm/arithmetic_dispatch` | 15.138–15.286 ms (15.202 ms estimate) |
| `vm/allocation_gc` | 8.380–8.729 ms (8.515 ms estimate) |
| `language/allocation` | 39.997–41.919 ms (40.853 ms estimate) |
| `language/arithmetic` | 127.73–131.13 ms (129.02 ms estimate) |
| `language/closures` | 95.829–98.888 ms (97.120 ms estimate) |
| `language/patterns` | 120.01–122.04 ms (120.83 ms estimate) |
| `language/string_bytes` | 43.381–51.867 ms (46.932 ms estimate) |
| `language/file_io` | 20.193–20.571 ms (20.378 ms estimate) |
| `language/channels` | 35.968–36.035 ms (36.001 ms estimate) |

Useful scale readings from those point estimates are about 16.4 million VM
arithmetic-loop iterations per second, 11.7 million retained heap allocations
per second, 3,140 cached small-file reads per second, and 278,000 channel
messages per second. These are workload throughput figures, not claims about
single-opcode or physical-disk limits.

The comparatively wide `language/string_bytes` interval makes a small claimed
improvement there untrustworthy. Any optimization targeting that case should
first rerun it with a longer measurement window.

## Capacity and whole-program checks

Capacity checks answer a different question from microbenchmarks and are run
manually when runtime limits, resident memory, or external IO change. Build the
release binary once, then measure a selected program with the platform's wall
time and peak-memory tool. On macOS:

```bash
cargo build --release
/usr/bin/time -l target/release/solveig --project-root examples/log_audit
```

The million-element shuffle in `examples/scratchpad` is a capacity case, not a
routine statistical benchmark. Its immutable decorate-sort-undecorate
algorithm has more than 4,194,304 simultaneously live heap objects, so the
default managed heap limit rejects it. Raising the explicit capacity for this
case allows it to complete without changing the default runtime policy:

```bash
SOLVEIG_HEAP_LIMIT=8388608 \
  target/release/solveig --project-root examples/scratchpad
```

One diagnostic run on the baseline machine completed in 16.07 seconds and
printed `1000000`. That single observation verifies capacity; it is not a
statistical performance baseline. A future contiguous array and Fisher–Yates
shuffle would be the appropriate performance design for million-element
shuffling, rather than tuning the list merge sort inside O1.

Network measurements must record whether they use the deterministic local
server or the public internet. Public-internet results are useful integration
checks but are not accepted as optimization evidence because remote latency
and congestion are uncontrolled.

## Phase O comparison policy

Every optimization records its exact before and after commands and confidence
intervals in `plan_v3/PROGRESS.md`. The affected benchmark is run immediately
before and after the change on the same machine. Full-suite runs are reserved
for cross-cutting changes and the phase exit gate. No optimization is accepted
from wall-clock intuition, debug builds, one-off `time` output, or a benchmark
whose result check fails.
