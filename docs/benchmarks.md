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
| `vm/global_load` | Reads one already-forced global from the same bytecode site during 250,000 integer loop iterations; this is O4's global inline-cache signal |

The `language` harness runs a real package through `solveig-driver`. One
`Compilation` is retained per case, so package-check caching is warm after the
first iteration, while the public check-to-run path, lowering, bytecode
emission, VM startup, and execution remain in every sample. These measurements
therefore show user-visible warm compiler/runtime performance rather than a
synthetic interpreter-only number.

| Benchmark | Work per sample |
|---|---|
| `language/compiler_pipeline` | Creates a fresh `Compilation`, checks and lowers the complete language benchmark package, starts the VM, and evaluates a trivial integer entry; this is O6's cold compiler-allocation signal |
| `language/allocation` | Builds 50,000 list cells and tail-recursively sums them |
| `language/arithmetic` | Runs 200,000 concrete `Int` iterations using typeclass operators and `mod`; this is the primary O2 signal |
| `language/constraint_generic` | Calls a user-defined `Num a` addition helper at `Int` for 200,000 iterations; this is the primary O3 residual-dictionary signal |
| `language/constraint_concrete` | Runs the equivalent loop through a monomorphic `Int` helper as O3's direct-call control |
| `language/closures` | Applies one capturing closure through a higher-order function 200,000 times |
| `language/static_closures` | Creates and invokes the same zero-capture lambda during 200,000 recursive iterations; this is O4's static-closure signal |
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

## Optimization results

O2 removed concrete method dictionary projection and lowered exact primitive
instance forwarders directly. The focused `language/arithmetic` estimate fell
from 126.33 ms immediately before the change to 46.521 ms afterward: 63.17%
less time, or 2.72x the throughput.

O3 added a paired user-defined constrained-function workload. Before
specialization, `language/constraint_generic` measured 53.029–54.300 ms
(53.566 ms estimate), while the equivalent monomorphic control measured
34.073–35.381 ms (34.617 ms estimate). After bounded specialization, the
generic case measured 34.880–35.042 ms (34.963 ms estimate), 34.73% less time
and statistically indistinguishable from the unchanged control. These O2 and
O3 runs used only their focused filters, following the policy below.

O4 measured each dispatch-hygiene item separately. Adding `#[repr(u8)]` and
the 16-byte layout guards left `vm/arithmetic_dispatch` statistically
unchanged: 15.263–15.346 ms (15.295 ms estimate) before and 15.333–15.430 ms
(15.376 ms estimate) after. `Value` and `Instr` are both currently 16 bytes.
A controlled release-mode checked-fetch variant measured 15.055–15.114 ms
(15.084 ms estimate), while the retained debug-asserted unchecked fetch
measured 14.997–15.026 ms (15.009 ms estimate), a 0.50% point-estimate
reduction below the benchmark's practical noise threshold.

The new global call-site cache reduced `vm/global_load` from 15.984–16.184 ms
(16.052 ms estimate) to 15.218–15.408 ms (15.323 ms estimate), 4.54% less
time. The zero-capture static representation already existed before O4; its
new regression workload compared a controlled allocating variant at
35.784–35.978 ms (35.875 ms estimate) with the retained `FunctionRef` path at
33.927–33.987 ms (33.955 ms estimate), 5.35% less time and 200,000 avoided
closure allocations. Only these focused cases ran; the full suite remains
reserved for the Phase O exit gate.

O5 first profiled one correctness-validation execution of every VM and
language case, counting only dynamically consecutive instruction offsets in
the same chunk. The dominant fusible triples were local/integer/binary
(3,780,255 executions), two-local/binary (910,000), and
two-local/known-call (450,008); local/known-call pairs occurred 1,508,697
times. The retained instruction set covers exactly those shapes. Lower-volume
generic, builtin, and tail-call combinations were not fused.

Focused same-session measurements before and after O5 were:

| Benchmark | Before | After | Point-estimate reduction |
|---|---:|---:|---:|
| `vm/arithmetic_dispatch` | 15.141–15.200 ms (15.167 ms) | 8.182–8.199 ms (8.191 ms) | 46.00% |
| `language/arithmetic` | 47.508–47.802 ms (47.631 ms) | 41.497–41.714 ms (41.599 ms) | 12.66% |
| `language/patterns` | 68.896–69.134 ms (69.012 ms) | 64.506–64.835 ms (64.665 ms) | 6.30% |

A controlled subtraction build kept the binary fusions but disabled the two
known-call forms. `language/patterns` then measured 66.838–67.579 ms (67.166
ms), a statistically detected 3.87% regression from the combined result, so
the call family independently earns its opcode surface. As with O2–O4, only
the affected statistical filters ran after profiling; the full statistical
suite remains reserved for the Phase O exit gate.

O6 profiled the collection lengths produced while checking and lowering the
complete language benchmark package. All 107 predicate argument vectors had
length one. Of 303 quantifier vectors, 211 were empty and 72 of the 92
non-empty vectors fit in two slots. Of 604 application spines, 561 fit in two
slots; 36 of 42 non-empty closure capture sets did too.

Inlining the AST vectors was rejected after measuring their representation:
`Ty` is 56 bytes, `SmallVec<[Ty; 1]>` is 72 bytes versus a 24-byte `Vec`, and
a two-quantifier `SmallVec` is 80 bytes versus 24 bytes. Persistent `CCExpr`
vectors were also retained because changing them would grow every node from 40
to 48 bytes. O6 instead uses two-slot work vectors only while collecting
application spines and capture sources, saving 597 heap allocations per
conversion without enlarging a retained IR node.

The focused cold `language/compiler_pipeline` benchmark fell from
68.599–70.726 ms (69.533 ms estimate) to 65.578–67.279 ms (66.321 ms
estimate), 4.62% less time. A four-slot controlled variant measured
65.609–69.574 ms (67.156 ms estimate) and had no statistically detected
improvement over two slots, so the smaller capacity was retained.

## Phase O exit: 2026-08-20

The complete statistical suite ran once after O6 and every required gate. The
machine, toolchain, Criterion configuration, and correctness policy match the
O1 baseline above.

| Benchmark | Phase O exit time |
|---|---:|
| `vm/arithmetic_dispatch` | 8.043–9.527 ms (8.711 ms estimate) |
| `vm/allocation_gc` | 8.157–8.236 ms (8.187 ms estimate) |
| `vm/global_load` | 14.877–15.124 ms (14.972 ms estimate) |
| `language/compiler_pipeline` | 65.126–65.480 ms (65.277 ms estimate) |
| `language/allocation` | 26.994–38.085 ms (31.851 ms estimate) |
| `language/arithmetic` | 43.936–44.805 ms (44.376 ms estimate) |
| `language/constraint_generic` | 27.848–28.190 ms (27.998 ms estimate) |
| `language/constraint_concrete` | 27.640–27.847 ms (27.732 ms estimate) |
| `language/closures` | 31.887–32.982 ms (32.424 ms estimate) |
| `language/static_closures` | 29.310–51.885 ms (37.497 ms estimate) |
| `language/patterns` | 64.647–65.244 ms (64.878 ms estimate) |
| `language/string_bytes` | 31.830–36.784 ms (33.599 ms estimate) |
| `language/file_io` | 20.333–20.585 ms (20.424 ms estimate) |
| `language/channels` | 31.474–33.495 ms (32.362 ms estimate) |

For the cases present in the original O1 suite, point-estimate time is lower
by 42.70% for VM arithmetic, 3.85% for VM allocation, 22.04% for source
allocation, 65.61% for source arithmetic, 66.61% for closures, 46.31% for
patterns, 28.41% for strings/bytes, and 10.11% for channels. File IO differs
by 0.23% in the slower direction, effectively unchanged. The wide exit
intervals for source allocation and static closures should not be used for a
small optimization claim; their focused task measurements remain the stronger
evidence where applicable.

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

## Phase U collection representation checks: 2026-08-20

U2 measured the first language-level collection representations against the
list encodings they replace. Each pair performs the same deterministic work
through the public driver and VM path; the result is validated before Criterion
samples it. These focused filters ran separately, not as a complete benchmark
suite:

```bash
cargo bench --bench language -- vector_index
cargo bench --bench language -- list_index
cargo bench --bench language -- 'map_lookup|association_lookup'
cargo bench --bench language -- 'language/(queue|naive_queue)'
```

| Workload | Retained representation | List control | Point estimate |
|---|---:|---:|---:|
| 2,000 indexed reads from 1,024 elements | Vector: 41.784 ms | List: 167.95 ms | Vector 4.02× faster |
| 2,000 ordered-key lookups in 1,024 entries | AVL Map: 51.585 ms | Association list: 151.44 ms | Map 2.94× faster |
| Enqueue and drain 1,000 elements | Queue: 33.429 ms | Append-list FIFO: 83.304 ms | Queue 2.49× faster |

These are representation-selection results, not claims that the current
language-level trees match a native array or hash table. The retained APIs
state their actual bounds: logarithmic Vector indexing/update/right append,
logarithmic ordered Map/Set operations, and amortized constant-time Queue ends.

## Phase U JSON scanning check: 2026-08-20

U3.2 compared its general allocation-free byte-range primitive with the scalar
`bytesIndex : Bytes -> Int -> Maybe Int` loop it replaces in JSON digit runs.
Both cases scan the same 256 bytes 500 times and validate the same result. Only
the focused filter ran:

```bash
cargo bench --bench language -- json_scan
```

`language/json_scan_bulk` measured 61.543–62.003 ms (61.766 ms estimate).
`language/json_scan_scalar` measured 140.50–155.02 ms (146.76 ms estimate).
The bulk boundary is therefore 2.38× faster by point estimate for this
workload. Each sample still includes the shared warm check-to-run path,
lowering, VM startup, and teardown, so this is conservative evidence for the
scanner choice rather than a claim about isolated byte throughput. The full
benchmark suite remains reserved for U5.

## Phase O comparison policy

Every optimization records its exact before and after commands and confidence
intervals in `plan_v3/PROGRESS.md`. The affected benchmark is run immediately
before and after the change on the same machine. Full-suite runs are reserved
for cross-cutting changes and the phase exit gate. No optimization is accepted
from wall-clock intuition, debug builds, one-off `time` output, or a benchmark
whose result check fails.
