# Solveig VM

This document is the contract for Solveig's bytecode VM. It describes the
state established by Phase I and the invariants that later REPL, error,
concurrency, IO, and performance work must preserve. The VM is a strict,
stack-based eval/apply machine with green processes, one continuation stack,
and a generational tracing collector.

`solveig-vm` owns bytecode emission from closure IR, instruction execution,
runtime values, heap allocation and collection, processes, scheduling,
builtins, and host IO execution. It does not own source text, name resolution,
type checking, or diagnostics. The driver selects an entry global and supplies
host streams; the VM returns `EvalResult` or `RuntimeError`.

## From closure IR to execution

`lower_to_bytecode` turns a `CCProgram` into a `BytecodeProgram`. The program
contains dense chunks, global-to-chunk entries, constructor metadata, and
string pools. There are two chunk kinds:

- `Function { capture_count, arity }` is executable function code. Its frame
  slots begin with captures, then arguments in oldest-to-newest order, then
  compiler-allocated local and pattern-binding slots.
- `GlobalThunk` computes one top-level value. `GlobalSlot` separately tracks
  whether that thunk is unevaluated, owned by an evaluator, evaluated, or
  failed.

Each chunk contains a flat `Vec<Instr>`, resolved jump targets, side tables for
pattern matching, its maximum frame slot count, and an always-built source
table. The bytecode-offset/source-span invariant and future debugger boundary
are detailed in [debugging-hooks.md](debugging-hooks.md).

The instruction set has these groups:

| Group | Instructions | Stack and control effect |
|---|---|---|
| Constants and references | `PushUnit`, `PushInt`, `PushFloat`, `PushChar`, `PushStr`, `PushCtorFn`, `PushFunctionRef` | Push one immediate or allocated value. A nullary constructor uses the runtime cache. |
| Slots and globals | `LoadLocal`, `StoreLocal`, `LoadGlobal` | Load/store a frame slot, or force and memoize a global thunk. |
| Computation and allocation | `Unary`, `Binary`, `MakeClosure`, `Pack`, `CallBuiltin` | Consume operands and push one result; allocating cases pass through the allocation safe point. |
| Calls and returns | `CallN`, `CallKnown`, `TailCallN`, `TailCallKnown`, `Return` | Enter or reuse a frame according to eval/apply; `Return` resumes the top continuation. |
| Branching and matching | `Jump`, `JumpIfFalse`, `Match`, `Trap` | Set the program counter, select a match-table arm and bind its slots, or produce `RuntimeError`. |

Bytecode is trusted compiler output. Dispatch uses unchecked accesses only at
sites where emission, jump patching, and frame sizing establish the bounds;
the accompanying safety comments state those invariants. Source type and shape
errors are rejected before this boundary, while dynamic faults such as division
by zero become tagged defects transported through the dispatch result. A
malformed emitted instruction is an internal compiler error, not a recoverable
language error.

## Runtime values

`Value` is a `Copy` enum, measured at 16 bytes on the I3 target. It contains
either an immediate value or one opaque heap location:

| Variant | Meaning |
|---|---|
| `Unit`, `Int`, `Float`, `Char` | Primitive strict values. Integer arithmetic uses `i64`; equality and hashing compare floats by their bit pattern. |
| `ThreadId(u32)` | Opaque language-visible green-process identity. It is not a heap location and has no numeric language operations. |
| `CtorFn { tag, arity }` | Unsaturated constructor function. Nullary constructors are represented by cached `Data0` objects instead. |
| `FunctionRef { chunk, arity }` | Capture-free callable function. |
| `Obj(ObjId)` | Location of a managed `HeapObject`; valid only under the rooting and collection rules below. |

Heap objects hold everything whose representation is variable-sized, managed,
or callable with bound state:

| Object | Payload and role |
|---|---|
| `Closure` | Function chunk plus captured values in `PayloadStore`. |
| `PartialApp` | Function chunk, captures, and already-applied arguments. |
| `PartialCtor` | Constructor tag/arity plus already-applied fields. |
| `Data0`, `Data1`, `Data2`, `DataN` | Saturated algebraic data, with zero to two fields inline and larger payloads in `PayloadStore`. |
| `Str` | Immutable UTF-8 bytes in `ByteStore`. |
| `Chan` | Mutable FIFO channel state: side-payload queue, optional positive capacity, and closed flag. |
| `Bytes` | Immutable arbitrary bytes in the existing byte arena; unlike `Str`, the payload need not be UTF-8. |
| `IoAction` | Inert IO syntax interpreted only by the IO machine. |
| `Forwarded` | Temporary nursery forwarding entry, observable only while minor collection runs. |

`Value` equality is representation equality. In particular, two `Obj` values
compare their current locations; this is not structural data equality or
stable identity across allocation. Language-level structural operations must
inspect the represented objects or use their own typed identities.

## Eval/apply convention and frame layout

Solveig follows the eval/apply model described by Marlow and Peyton Jones,
“Making a fast curry: push/enter vs eval/apply” (ICFP 2004). A call evaluates
its callee and arguments before applying them. Immediately before application,
the arguments occupy the top of the value stack in source order; a generic
`CallN` removes the callee from directly beneath them.

Application dispatch distinguishes the callable representation:

- A capture-free `FunctionRef` or a `Closure` with exactly its remaining arity
  enters the function chunk.
- Too few arguments produce a `PartialApp`. Applying a `PartialApp` combines
  its captures and saved arguments with the new arguments; exact saturation
  then enters the function.
- `CtorFn` and `PartialCtor` use the same under/exact-application rule and
  allocate `Data0`/`Data1`/`Data2`/`DataN` when saturated.
- Closure conversion splits source over-application into successive calls.
  Reaching the runtime with more arguments than the callable's remaining arity
  is therefore a bytecode/lowering invariant failure.

For a normal call, `CallerFrame` saves the caller's chunk, resume PC, and frame
base. The callee frame begins at the first argument. Bound captures and
previously applied arguments are prepended, yielding this slot order:

```text
frame_base
  captures
  previously applied arguments
  new arguments
  compiler-allocated locals and pattern bindings
stack temporaries above the frame slots
```

`slot_count` reserves enough space for the chunk's complete frame. `LoadLocal`
and `StoreLocal` address `frame_base + slot`; de Bruijn locals are resolved to
those slots during bytecode emission.

A tail call does not push `CallerFrame`. It moves the new arguments down to the
current `frame_base`, truncates the old frame, changes chunk and PC, and sizes
the reused frame for the callee. This is why tail recursion uses bounded VM
stack. It also removes the logical tail-call frames from debugger inspection,
as recorded in `debugging-hooks.md`.

Native builtins are atomic with respect to scheduler return. Builtins may call
the same safe point for each allocation, and callback application uses an
explicit `Halt` continuation. No builtin or IO bind creates a Rust call chain
whose depth grows with program behavior.

## One continuation stack

Every `Process` owns exactly one `Vec<Continuation>`. It is heterogeneous by
design: bytecode return, thunk publication, and IO sequencing are all “what to
do with the next produced value.” There is no `IoFrame`, `io_stack`, exception
stack, or second caller stack. The current IO action is one `io_current`
register; suspended IO binds, typed-error handlers, and cleanup scopes live in
the continuation stack.

| Continuation | Meaning on completion |
|---|---|
| `Halt` | Return the produced value to an internal callback boundary. |
| `CallerFrame { chunk, pc, frame_base }` | Restore the ordinary bytecode caller and push the result on its value stack. |
| `PublishGlobalThenHalt { global_index }` | Publish the owned global thunk, wake waiters, and finish evaluation. |
| `PublishGlobalThenResume { global_index, chunk, pc, frame_base }` | Publish the thunk, restore the forcing caller, and push the value. |
| `IoBind(continuation)` | Apply the saved Solveig continuation to the completed IO result and continue with the returned action. |
| `IoCatch(handler)` | Handle the next typed failure below this frame; successful values pass through it. |
| `IoCatchDefect(handler)` | Handle a defect raised while executing the action below this installed scope. |
| `IoFinally(cleanup)` | Run cleanup after the action below finishes with a value, typed failure, defect, interruption, or program exit. |
| `IoRestore(outcome)` | Resume the saved success or typed failure after cleanup succeeds. |
| `IoRestoreDefect(defect)` | Resume defect unwinding after cleanup succeeds. |
| `IoRestoreMask(depth)` | Restore the previous scoped interruption-mask depth. |
| `IoRestoreInterruption { interruption, mask_depth }` | Restore masking and continue interruption unwind after cleanup succeeds. |
| `IoRestoreExit { status, mask_depth }` | Restore masking and continue program-exit unwind after cleanup succeeds. |

The enum is guarded at 40 bytes because `PublishGlobalThenResume` already sets
its hot-path size; the IO frames must not widen it. GC traces the `Value`
inside every IO frame. Global-owner failure walks the same stack to find
publication frames, so a second blackhole-ownership structure is forbidden.

The bytecode return path must see only bytecode/global continuations, and the
IO resume path must see only IO frames. The fixed `Halt` boundary bridges
callback execution back into the IO loop.

## `Runtime`, `Process`, and interpreter ownership

`Runtime` owns all state shared by green processes:

- immutable bytecode and constructor/string metadata;
- the single heap and global thunk table;
- initialized nullary-constructor and Boolean caches;
- the process arena and FIFO run queue;
- scheduler-owned FIFO channel sender and receiver wait queues;
- the scheduler-owned timer queue and monotonic clock;
- the scheduler-facing IO subsystem and reduction budget.

`Process` owns one green thread's mutable execution state and no heap memory:
its typed `ProcessId`, current chunk/PC/frame base, value stack, continuation
stack, pattern binding buffer, temporary roots, current IO action, pending
plain worker, channel, or timer result, status, and remaining reduction count. `ProcStatus` is
`Runnable`, `Running`, `Blocked(reason)`, `Finished(value)`, `Failed(error)`,
or `Interrupted(reason)`. A process also retains one pending interruption flag
and a nested mask-depth counter. Current block reasons are a worker `RequestId`,
a global `GlobalIndex`, a channel `ChanId`, and a timer `Deadline`; later
resources must extend this typed enum rather than encode meaning in integers.

The scheduler removes an active process from the arena for the duration of a
step and leaves a placeholder at its id. This gives the interpreter an ordinary
`&mut Process` alongside disjoint mutable borrows of shared runtime fields,
without `RefCell` or a synchronized heap. `Interpreter<'_>` is only that
short-lived split-borrow view; it owns no state.

The free function `run(&mut Runtime, &mut Process) -> StepOutcome` replenishes
an exhausted budget, marks the process running, executes the dispatch loop,
and records the explicit scheduler return reason. `StepOutcome` is `Yielded`,
`Blocked`, `Finished(Value)`, `Failed(RuntimeError)`, or `Interrupted`, following the return
reason discipline in GHC's scheduler. The IO path uses the same outcome and
process lifecycle while its outer loop interprets `IoAction` values.

## IO actions and sequencing

`IoAction` is data, not an eagerly performed host effect. Its variants are
`Pure`, typed `Fail`, `Bind`, `Catch`, `CatchDefect`, `Finally`, terminal
print/read, `GetArgs`, `Spawn`, `KillThread`, `Sleep`, scoped mask operations,
channel operations, and file read/write. The IO loop keeps the
current action in `Process::io_current` and pushes the matching IO continuation
when descending through a compound action. When an action yields a value, the loop
pops the saved continuation, applies it through bytecode when required, and
treats the result as the next action. This is the operational IO sequencing
model from Peyton Jones, “Tackling the Awkward Squad,” section 2.

When the loop reaches `Fail`, it discards bind frames until it finds
`IoCatch`, runs `IoFinally` cleanup on the way out, or returns the inert failure
at the process boundary. Typed failure therefore short-circuits sequencing
without using `RuntimeError` or defect unwinding. If cleanup succeeds,
`IoRestore` resumes the saved success or typed failure; a cleanup failure
replaces the saved outcome. An unhandled typed error becomes a reported
process failure only after it escapes this IO machinery.

Defects are tagged separately from infrastructure `RuntimeError`s while pure
bytecode evaluates. At an installed `IoCatchDefect` or `IoFinally` boundary,
the tag becomes the matching closed Prelude `Defect` value. Unwinding discards
bytecode callers, typed-error handlers, and binds in their actual continuation
order; it fails owned global thunks, runs `IoFinally` cleanup, and enters only a
defect handler. A cleanup defect replaces the saved defect. Infrastructure
errors never enter this path, and an uncaught defect becomes a process failure
at the scheduler boundary, so a spawned defect terminates and reports only its
own thread.

The private `RaiseDefect` IO action resumes this same unwind from an already
materialized Prelude `Defect` value. Structured concurrency workers use it to
transport a child's exact defect through a private channel and re-raise it in
the parent; it is not a second exception mechanism.

Worker-backed actions park the process. Their completion is plain Rust data in
`io_result`; it becomes a `Value`, including any heap allocation for returned
text, only after the scheduler wakes that process. `Spawn` is not a host worker
operation: it allocates a new green process whose `io_current` roots the child
action and appends it to the FIFO run queue. The child inherits the parent's
mask depth so Prelude code can install cleanup before explicitly restoring the
child action's interruption state.

## Generational heap layout

The heap uses fixed-size, index-addressed object slots. It does not expose raw
pointers and the collector is written entirely in safe Rust.

The object arena is split into two ranges:

- Nursery ids are in `0..nursery_slot_count`. The nursery is a
  `Vec<HeapObject>` with capacity `nursery_slot_count`; its length is the bump
  index. Allocation checks the limit, then pushes one object.
- Old-generation ids start at `nursery_slot_count`. Subtracting that base gives
  an index into a `Vec<Option<HeapObject>>`; major collection reuses vacant old
  slots through a free list.

The range boundary belongs to `Heap`; `ObjId` remains one opaque `u32` index.
No generation tag or other data is packed into its bits. Code outside the heap
must ask the heap to resolve an id rather than depending on either range.

The nursery slot count is fixed when a heap is created. A minor collection
clears the nursery only after it has rewritten all reachable references. Old
objects do not move: major collection remains mark-sweep, without compaction.

### Measured layout

The I3 baseline was measured on `aarch64-apple-darwin` with:

```text
RUSTC_BOOTSTRAP=1 cargo rustc -p solveig-vm --lib -- -Zprint-type-sizes
```

| Type or variant | Size |
|---|---:|
| `HeapObject` | 48 bytes, alignment 8 |
| `Option<HeapObject>` | 48 bytes |
| `IoAction` | 40 bytes, alignment 8 |
| `Value` | 16 bytes, alignment 8 |
| `ObjId` | 4 bytes, alignment 4 |
| `PayloadRef` / `ByteRef` | 16 bytes each, alignment 8 |
| `HeapObject::PartialApp` payload plus discriminant | 46 bytes |
| `HeapObject::IoAction` payload plus discriminant | 46 bytes |
| `HeapObject::Data2` payload plus discriminant | 38 bytes |

`PartialApp` and `IoAction`, not `Data2`, currently determine the 48-byte
uniform slot size. Adding `Forwarded(ObjId)` does not widen it. Nursery sizing
must nevertheless use `size_of::<HeapObject>()`; these measurements describe
the current target and are not a portable constant.

The representation intentionally keeps `Data0`, `Data1`, and `Data2` fields in
the object slot. `DataN`, `Closure`, `PartialApp`, and `PartialCtor` keep their
variable-length values in `PayloadStore`. `Str` keeps its UTF-8 bytes in
`ByteStore`. The inline cases therefore take one object lookup; the side-store
cases take a second lookup. Changing that split requires Phase O profile data.

## Forwarding and minor collection

`HeapObject` has a `Forwarded(ObjId)` variant. A forwarding entry is valid
only while a minor collection is running and only in a vacated nursery slot.
Normal object lookup must treat observing one as an internal invariant failure.
The old generation must never contain a forwarding entry after collection.

When the allocation limit fails, minor collection proceeds as follows:

1. Visit every root mutably. For each nursery `ObjId`, copy its object into an
   old-generation slot, replace the nursery slot with `Forwarded(new_id)`,
   rewrite the root, and enqueue the promoted object. If the nursery slot is
   already forwarded, reuse its recorded destination.
2. Drain the promoted-object worklist. Rewrite every direct `Value` field and
   every value in a copied payload by the same evacuation operation. This
   transitively promotes the reachable nursery graph. Young-to-old edges need
   no special treatment.
3. Scan every old channel in the remembered set, evacuate nursery values in its
   queue, rewrite those payload entries, and clear the set for the next nursery
   epoch.
4. After no nursery reference remains in roots, promoted objects, or remembered
   old objects, clear the nursery object, payload, and byte arenas together.
5. If the old-generation threshold is also due, run the existing mark-sweep
   major collection now, while the nursery is empty.

The forwarding variant is preferred over a parallel map because object state
and forwarding state cannot diverge. A second path reaching the same survivor
always sees the forwarding entry and copies neither the object nor its side
payload twice. This is the standard forwarding-pointer copying algorithm from
Jones, Hosking, and Moss, *The Garbage Collection Handbook*, chapter 4; the
generational organization follows Appel, “Simple generational garbage
collection and fast allocation” (1989).

## `ObjId` lifetime

> An `ObjId` identifies a slot in the current heap epoch, not a stable object
> identity. Any allocation is a possible collection point. Caching an `ObjId`
> or a `Value::Obj` across a possible allocation without putting it in `Roots`
> is a correctness bug.

Minor collection rewrites every live nursery id to its new old-generation id.
The cleared nursery indices are reused by the next epoch, so a stale nursery id
can silently name an unrelated new object rather than merely becoming invalid.
Old-generation ids currently do not move, but callers must not specialize
their behavior around that fact. Equality and hashing of `ObjId` are meaningful
only inside a non-allocating epoch. Long-lived runtime identity uses its own
typed ids, such as `ProcessId`; it never uses an object location.

Collector entry is an allocation safe point. Before entering it, interpreter
temporaries that contain `Value`s must have been published into the active
`Process` or its `temporary_roots`. The one `Roots` implementation then rewrites
all of these locations:

- the active process and every inactive process: value stack, binding buffer,
  temporary roots, finished result, current IO action, pending channel result,
  channel block reason, and every value-bearing IO continuation;
- scheduler channel-waiter map keys and every parked sender value;
- every `GlobalSlot::Evaluated` value;
- every nullary-constructor-cache value and both Boolean cache values.

`Roots` must expose mutable traversal for relocation and remain the sole root
enumeration used by minor collection, major collection, and the debug
post-collection heap walk. Adding any field that can retain a `Value` across an
allocation requires adding it there in the same change. Publishing a local copy
into `temporary_roots` does not rewrite the local itself: after a possible
collection, code must reload the value from its rewritten root slot before
using it. No allocation path, including builtins that allocate more than one
object, may bypass this safe-point and reload discipline.

## Payload and byte arenas

`PayloadStore` and `ByteStore` are generational too. Each owns a bump-appended
nursery arena and a reclaiming old arena. A non-empty `PayloadRef` or `ByteRef`
is owned by exactly one heap object, as required already by major collection's
per-object freeing. Empty spans are the only shared spans.

Side-store references remain ranges rather than independent object identities.
Their generation is the generation of their owning heap object; a bare side
reference is not self-describing. Heap APIs that read a span must therefore
retain or receive the owning object's location. A side reference copied into a
Rust local is valid only until the next possible allocation.

Object construction appends the side payload first and publishes its nursery
object immediately afterward, with no collection point between those actions.
When an object is promoted, the collector copies each of its non-empty payload
spans into the corresponding old arena and rewrites the promoted header to the
new range. `PayloadStore` values are evacuated while being copied; `ByteStore`
bytes are copied exactly. Once all survivor headers refer to old spans, all
three nursery arenas can be reset in constant time. Major collection continues
to return the old spans of dead objects to their old-arena free lists.

Keeping the stores generational is mandatory: resetting only object slots would
leave side payloads growing without bound, while resetting side stores without
rewriting promoted headers would create dangling ranges.

## Write-barrier inventory — any task adding mutation must add its barrier here

A remembered-set barrier is required whenever a mutable old heap object can be
made to reference a nursery object. Missing an entry from this inventory is a
collector correctness bug; chapter 9 of *The Garbage Collection Handbook* is
the reference for the barrier and remembered-set discipline.

The Phase I inventory is:

- Ordinary heap objects are immutable after publication. Their remembered-set
  mutation sites are therefore empty.
- Publishing a thunk result changes `GlobalSlot` from `Evaluating` to
  `Evaluated(Value)` and wakes its blackhole waiters. Globals are scanned on
  every minor collection as roots, so this site needs no remembered-set
  insertion today, but publication must remain a single audited operation and
  is part of the barrier inventory. Publishing failure stores no heap value.
- Process stacks, process status, continuations, constructor caches, and Boolean
  caches are roots rather than old heap objects. Their mutable `Value` slots are
  rewritten by `Roots` and do not enter the remembered set.
- `Heap::push_channel` is the sole channel queue publication site. It replaces
  the channel's side-payload range and inserts the channel into
  `remembered_channels` whenever an old channel receives a nursery object.
  Direct handoff to a waiting receiver and parked sender values live in roots,
  not in an old heap object, so they require no remembered-set entry.

Promotion itself cannot create an old-to-young edge because the collector
evacuates all children before completing the minor collection. New immutable
objects may point from young to old, which needs no barrier.

## Global thunk blackholes

A process that starts evaluating a global claims its slot eagerly as
`Evaluating { owner, waiters }`. This follows the eager-blackholing protocol in
Marlow, Peyton Jones, and Singh, “Runtime Support for Multicore Haskell” (ICFP
2009), sections 8 and 8.1.

- The owner reaching the same slot again is a genuine global cycle and fails
  with `RuntimeError`.
- A different process reaching the slot becomes
  `Blocked(Blackhole(GlobalIndex))` and remains parked at its `LoadGlobal`
  continuation point.
- Successful publication changes the slot to `Evaluated(Value)`, pushes that
  value onto every waiter’s stack, and places the waiters back on the FIFO run
  queue.
- Owner failure changes every global it was evaluating to `Failed(error)` and
  transfers the same failure to all waiters. This propagation is transitive:
  if a failed waiter owned an outer global, that blackhole and its waiters fail
  too. Later attempts to force the global observe the cached failure rather
  than retrying or blocking forever.
- Owner interruption restores each claimed slot to its original thunk and
  wakes waiters at their `LoadGlobal` instruction so one can retry. Unlike a
  deterministic defect, interruption must not be memoized.

The process continuation stack is the ownership record: each live
`PublishGlobalThen*` frame identifies one global the process must either
publish, fail, or restore after interruption. There is no second per-process
blackhole stack.

## Safe points and preemption

`Interpreter::safe_point` is the only runtime boundary that consumes a
process reduction and admits GC. Calls, taken backward jumps, and allocation
paths all reach this function. Exhausting its reduction budget returns to the
scheduler; interruption is checked before that same suspended process resumes,
so no second polling cadence exists. The IO machine also checks pending
delivery before each action and when a masked operation is about to park.

The reduction that exhausts a quantum is allowed to finish its current
bytecode instruction before `run` returns `StepOutcome::Yielded`. Consequently
the saved program counter always identifies the next instruction, heap results
have been published into a root, and call/return continuations are complete.
Native builtins remain atomic scheduler operations; builtins that allocate in
a loop still reach the same safe point for every allocation, but return to the
scheduler only after the builtin has restored its rooted temporary state.
Resuming a yielded process replenishes its budget and continues from the saved
state. IO callback evaluation uses the same path and retains its pending
`IoBind` state across a yield.

The default quantum is 4,096 reductions. On the five-million-call tail-loop
fixture, budgets of 4,096, 65,536, and `u32::MAX` each completed in 1.54 seconds
of test time on the I6 development machine; a budget of 256 also took 1.54
seconds but incurred more scheduler-boundary traffic. The smallest candidate
with no measured interpreter penalty was selected to keep preemption latency
bounded without optimizing against noise. `SOLVEIG_REDUCTIONS=<positive u32>`
overrides it; zero, malformed, and out-of-range values fall back to the default.

The run queue is a strict `VecDeque` FIFO, and every yielded process rejoins at
the back. This is the deterministic policy used by tests. Setting
`SOLVEIG_SCHED_DETERMINISTIC=1` explicitly pins that contract; I6 has no
alternative scheduler policy, and the I7 loop deliberately preserves it, so
the setting remains idempotent. This follows the AOSA GHC runtime
chapter's rule that lightweight-thread context switches occur only where VM
state is tidy, while adding explicit call and backward-jump points so a pure
non-allocating loop cannot starve the run queue.

Every yielded process also retains a walkable bytecode frame stack, and every
chunk keeps its bytecode-offset source table in all builds. The exact hooks,
resume-PC convention, tail-call limitation, and work still required for a DAP
are specified in [debugging-hooks.md](debugging-hooks.md).

## Scheduler, `spawn`, and program lifetime

Solveig runs lightweight processes on one OS thread. The scheduler removes the
front runnable process, runs its IO machine until it yields, blocks, finishes,
or fails, and then re-queues, parks, or retires that process. A spawned process
therefore shares the VM heap and global table but owns its instruction state,
stacks, continuation frames, status, and reduction budget. This is the
lightweight-thread model described in Peyton Jones, Gordon, and Finne,
“Concurrent Haskell” (POPL 1996), with the explicit scheduler return reasons
described by the AOSA GHC runtime chapter.

The scheduler transition for each `StepOutcome` is fixed:

| Outcome | Process and queue transition |
|---|---|
| `Yielded` | Mark `Runnable` and append the process to the FIFO tail. |
| `Blocked` | Retain the process as `Blocked(reason)` and leave it out of the run queue until the matching resource wakes it. |
| `Finished(value)` | Retain the finished value as a GC root. Main completion returns it and ends scheduling; a non-main process remains finished and is otherwise ignored. |
| `Failed(error)` | Fail any globals owned by the process and clear its resumable state. Main failure returns the error; non-main failure is written to stderr. |
| `Interrupted` | Revert interrupted global evaluations, run installed cleanup while unwinding, clear resumable state, and retire the process without reporting cancellation as an error. |
| `Exited(status)` | Revert owned global evaluations and unwind installed cleanup. A child is retired; main triggers sibling shutdown and returns the status to the host boundary. |

`spawn : forall childError e. IO childError () -> IO e ThreadId` constructs an
inert IO action.
Performing that action allocates a process, puts its child action in the process
root set, and appends it to the FIFO run queue. `ThreadId` is an opaque
primitive type: its runtime payload identifies the process arena entry, but the
language exposes no numeric operations or representation conversion for it.

Program lifetime is owned by the main process:

- When main finishes successfully, the scheduler returns its result and the
  runtime is dropped. Any other runnable or blocked Solveig processes are
  killed; their pending IO completions are not resumed. The scoped host worker
  threads are joined before a library-backed execution call returns, so an
  already-running host call is allowed to finish without touching VM state.
- When main fails, that `RuntimeError` terminates the program.
- When `exitWith` escapes main, the scheduler interrupts every live sibling
  once and drains its cleanup before returning the requested `ExitStatus`.
  Runtime teardown then joins the IO workers and flushes every still-open
  output handle. A child-thread exit retires only that child.
- When a non-main process fails, the scheduler writes
  `thread <id> failed: <error>` to stderr, retires that process, and continues
  running the program. The failure does not propagate to main.

The ordering contract remains deterministic FIFO. `SOLVEIG_REDUCTIONS` fixes
the quantum used by every process, and `SOLVEIG_SCHED_DETERMINISTIC=1` selects
virtual time for tests. Normal execution reads only monotonic elapsed time;
deterministic scheduling never reads or waits for wall-clock time.

## Interruption and masking

`killThread : ThreadId -> Task ()` constructs an inert request. Performing it
sets the target process's single pending `Interruption::Killed` flag and returns
without waiting for delivery. A finished, failed, interrupted, or unknown
identity is a successful no-op. A runnable target accepts the request when it
next resumes at the existing safe-point cadence. This follows Marlow, Peyton
Jones, Moran, and Reppy, “Asynchronous Exceptions in Haskell” (PLDI 2001),
sections 5.2–5.3 and 8.

`mask` is scoped and nestable. Entering it increments `Process::mask_depth`;
an `IoRestoreMask` frame restores the previous depth on success, typed failure,
defect, or interruption. While the depth is non-zero, ordinary safe-point
delivery is deferred. Reaching zero delivers a pending request before the next
action continues.

The paper's interruptible-operation rule still applies inside a mask. A channel
or worker operation that can complete immediately does so atomically. If it
must park, a pending interruption wins; if it is already parked, `killThread`
removes its channel waiter or cancels its request-table registration and wakes
it to unwind. The host call itself cannot be forcibly stopped, but its eventual
plain-Rust completion is discarded and cannot wake or mutate the retired
process. Timer waits use the same cancellation path: interruption removes the
exact scheduler registration before waking the target to unwind.

`finally` and `bracket` are Prelude compositions over private mask-state IO
actions. Their protected body runs with the prior state restored; cleanup runs
masked. Interruption unwinding ignores typed-error and defect handlers, executes
each installed `IoFinally`, and resumes the saved interruption after cleanup
succeeds. A cleanup typed failure or defect replaces the interruption, matching
the existing cleanup-precedence rule. No uninterruptible mask is exposed: the
first interruption cannot land in runnable cleanup, while a later request may
still stop cleanup at an indefinitely blocking operation.

Program exit is a separate synchronous `StepOutcome::Exited(ExitStatus)`.
Its unwind likewise skips typed-error and defect handlers, runs `IoFinally`
cleanup masked, and resumes through `IoRestoreExit`. Main exit then uses the
ordinary interruption machinery to cancel siblings; each sibling is signalled
only once so shutdown cannot cancel the cleanup it just entered.

## Timers and virtual time

`sleep : Int -> Task ()` is an inert action. Performing a positive duration
registers the current process in the scheduler-owned `TimerQueue` and parks it
as `Blocked(Timer(deadline))`; non-positive durations complete immediately.
`Deadline` is a monotonic millisecond instant local to one runtime, not a wall
clock timestamp. Entries are ordered by deadline and then `ProcessId`, giving
simultaneous wakeups deterministic process-creation order.

Normal execution derives deadlines from `Instant` and, with no runnable
process, waits on the IO completion receiver only until the earliest deadline.
`std::sync::mpsc::Receiver::recv_timeout` supplies that two-way wait, so the VM
does not need a selection dependency. If no worker request can wake a process,
the scheduler waits directly until the deadline.

Captured execution and `SOLVEIG_SCHED_DETERMINISTIC=1` use a logical clock.
When the run queue is empty, the clock jumps to the earliest registered
deadline and wakes every timer now due. Tests therefore exercise ordering,
masking, cleanup, and cancellation without host sleeps or elapsed-time
assertions. This preserves invariant I9 while using the same timer queue and
process transitions as real execution.

The same `TimerClock` also serves the inert `getMonotonicTime` and
`getSystemTime` actions, so the VM has no competing clock origin. Both return
nanoseconds as saturating `Int` values. In normal execution, monotonic time is
elapsed `Instant` time from runtime creation and system time is read from
`SystemTime` relative to `UNIX_EPOCH`. In virtual execution, monotonic time and
system time both equal the logical elapsed duration, making the virtual system
clock begin at the Unix epoch. Clock reads neither park the green thread nor
cross the worker pool.

## Explicit pseudorandom state

`StdGen` is an immediate opaque `Value` carrying one SplitMix64 seed. A pure
`randomInt` transition adds the fixed golden-gamma increment, applies the
published `mix64` permutation, and returns the signed output with the new
generator in a Prelude 2-tuple. The public `std.random.randomRange` returns
`Nothing` for reversed bounds before its primitive maps a word into inclusive
bounds using multiply-high rejection sampling rather than biased modulo. A
valid draw may advance through more than one word.

`newStdGen` is an inert nullary IO action. `Runtime` owns one `RandomSeeder`,
initialized from its `TimerClock` system reading, and performing the action
advances only that runtime-local stream. The action neither allocates host
state nor crosses the worker pool. Virtual clock mode starts the seeder at
zero, giving captured tests a fixed sequence. The generator is intentionally
non-cryptographic; runtime locality prevents global mutation, not seed
prediction.

## Typed channels

`Chan a` is a compiler-known abstract type whose values point to mutable
`Chan` heap objects. `newChan` creates an unbounded queue and quantifies its
typed error parameter so it composes contextually with any `IO e`. The public
`std.channel.newBoundedChan` requires a capacity from 1 through `u32::MAX` and
raises `InvalidChannelCapacity capacity` as a typed `IOError` before creating
the primitive action.

`send` hands a value directly to the oldest waiting receiver, appends it to the
FIFO queue when capacity permits, or parks the sender when a bounded queue is
full. `recv` removes the oldest queued value or parks the receiver while an
open queue is empty. Removing a value from a full bounded channel admits and
wakes the oldest parked sender. `tryRecv` performs the same dequeue without
parking and returns `Nothing` only for an open empty channel.

`closeChan` is idempotent and retains values already queued. Once those values
are drained, `recv` and `tryRecv` fail with `ChannelClosed`; `send` fails with
the same typed `IOError` immediately. Closing wakes every parked sender and
receiver with that failure. Sender and receiver waiter queues are independently
FIFO, as are queued values. Wait queues live in `Runtime`, not the heap object:
their channel keys and parked sender values are roots rewritten by collection.

## Blocking IO workers

Blocking host IO does not run on the scheduler thread. `Runtime` owns an
`IoSubsystem` with four scoped OS workers, one readiness-driven socket thread,
a typed request table, one host-handle side table distributed across those
workers, and one multi-producer/single-consumer completion channel. Performing
a handle operation, whole-file text/byte operation, `appendFile`, a
potentially blocking filesystem operation, or captured child process follows
this state transition:

1. The scheduler thread resolves every VM operand to owned Rust data and
   submits an `IoRequest` associated with the current `ProcessId`.
2. The process records `Blocked(Io(RequestId))` and leaves the run queue.
3. A worker or the socket event loop performs the host operation and returns
   only owned text/bytes, a `HandleId`, a `SocketId`, unit, or a contextual `io::Error`.
   Host threads never hold `Value`, `ObjId`, heap roots, or any other VM
   reference.
4. The scheduler drains completions, verifies that request and block reason
   match, stores the plain result on the process, and returns it to the FIFO
   run queue. That process converts text into a heap string at its next
   allocation safe point on the scheduler thread. Bytes likewise become an
   immutable heap byte string only after wakeup. A host error becomes the
   matching closed `IOError` constructor and an inert `Fail` action there; it
   does not enter the VM defect channel.

If interruption cancels a blocked request, its request-table entry becomes a
cancelled tombstone. A blocking worker completion eventually frees it; the
socket loop instead removes the exact readiness registration and immediately
acknowledges cancellation. A process request additionally signals its owning
worker to terminate and reap the direct child before completing. Only a
still-owned request can wake a process.

The VM enforces managed process-stack and heap-object ceilings. The defaults
are intentionally above the million-bind regression; tests and embedders may
lower them with `SOLVEIG_STACK_LIMIT` and `SOLVEIG_HEAP_LIMIT`. Reaching the
heap ceiling first runs a full collection and raises `HeapExhausted` only if
the retained object count still meets the limit. Defect constructors are
available from the startup constructor cache, and the handler or cleanup is
rooted across the recovery collection.

Completed request-table slots are reused only after their completion has been
consumed, so a `RequestId` identifies at most one outstanding operation. The
table contains the process association rather than encoding it in the id.

Terminal reads and writes share worker zero and therefore retain submission
order on the caller-provided streams. File, filesystem-metadata, directory,
and DNS requests are distributed round-robin across the other three workers
and may finish in a different order;
each completion wakes only its requesting process. DNS produces an owned IPv4
address string before `connect` submits it to the socket thread. Stream
references are borrowed by scoped workers and protected by `Mutex` because
multiple worker threads exist, but the mutex does not protect compiler or VM
heap state.

The subsystem snapshots a UTF-8 environment and canonical project-root cwd at
startup. Both remain runtime-local mutable state on the scheduler thread.
Environment reads and writes complete immediately; filesystem paths are
resolved against the logical cwd before an owned request crosses to a worker.
Successful `setCurrentDirectory` returns a canonical host path which the
scheduler installs only after wakeup. This avoids unsafe host-global
environment or cwd mutation when multiple workers and runtimes exist.

Captured process requests copy the executable, argument vector, logical cwd,
complete runtime-local environment, and optional per-stream byte limit before
crossing the worker boundary. The worker invokes no shell, closes stdin, owns
the direct child through termination and reap, and drains both output pipes
concurrently. The scheduler later allocates the three-field `ProcessResult`.
A non-zero status is data, while spawn/wait/UTF-8/limit failures map to
`RunProcessError`. Cancellation tombstones the language request and signals
the worker to terminate and reap the child before posting the discarded
completion.

Dropping the IO subsystem first signals every active child-process request,
then closes every worker queue. Each worker terminates and reaps its active
child and flushes all open output handles before its scoped host thread joins;
this also happens when its completion receiver has already closed during
program exit.

TCP resources live only in the socket thread's monotonic side table keyed by
`SocketId`. A Solveig `Socket` value is the typed id, never an OS descriptor or
a heap slot, so copying collection cannot move or duplicate the resource.
`mio::Poll` registers interest only while an accept, connect, send, or receive
is pending. Readiness is treated as a hint: operations drain until
`WouldBlock`, partial writes retain their owned buffer and offset, and
spurious connect readiness is checked with `take_error` and `peer_addr`.
All socket completions enter the same request table and scheduler wakeup path
as blocking IO. Explicit close removes the side-table entry and wakes any
outstanding operations with their specific typed error; ids are not reused,
so a stale closed `Socket` cannot alias a later OS resource.

Every scheduler iteration first drains ready completions. If the run queue is
empty while IO requests remain, it blocks on the completion receiver rather
than polling or sleeping. If the queue is empty and no request can complete,
execution fails with `all threads are blocked forever`. This is the external
completion/deadlock distinction used by GHC's scheduler, described in the AOSA
GHC runtime chapter. Standard-library per-worker `mpsc` queues express this
I8 topology; Phase J's requirement to wait on either a completion or a timer
deadline is the point at which a selecting channel may become necessary.

## Contributor invariants

These are correctness constraints, not current implementation suggestions.
Any change that breaks one needs an explicit replacement design and a recorded
owner decision.

### Execution and control

- The VM remains the only execution target and remains a stack machine using
  eval/apply. Do not add ANF, a register VM, native code, or a second evaluator.
- A process has one value stack and one heterogeneous continuation stack.
  Returns, global publication, IO bind/error/cleanup scopes, interruption, and
  debugger frame walking must agree on that order. Do not add a parallel
  control stack.
- Interpreter and IO loops must not recurse on the native stack in proportion
  to Solveig calls, bind depth, or process count.
- `Runtime` owns the heap, globals, processes, run queue, and IO subsystem.
  `Process` owns only per-thread execution state. Workers never receive either
  type or any borrowed VM value.
- The compiler establishes bytecode stack, slot, arity, and jump invariants.
  User-triggerable failures are `RuntimeError`; broken trusted bytecode is an
  ICE with a stated invariant.
- Normal calls preserve caller frames. Tail calls are the only current frame
  elision, and their debugger consequence is documented. Any new frame
  collapse must update `debugging-hooks.md`.
- Every emitted instruction has a corresponding source-table row in every
  build. Synthetic instructions use `None`; source mapping is not rebuilt by
  inspecting bytecode.

### Roots and allocation

- Any allocation may collect and relocate nursery objects. Before reaching an
  allocation safe point, every live `Value::Obj` must be in `Roots`.
- A Rust local copied into `temporary_roots` is not itself rewritten. Reload it
  from the root slot after a possible collection before using it again.
- `Roots` is the only enumeration used by minor collection, major collection,
  and the debug liveness walk. Adding a process, cache, continuation, global,
  or runtime field that retains `Value` across allocation requires updating
  `Roots` in the same change.
- Heap objects are immutable after publication unless their mutation and
  barrier are added to the write-barrier inventory above. Heap and collector
  state stay on the scheduler thread; making the heap synchronized would
  violate the runtime ownership model.

### Collector invariants

- Allocation and collection use indices and checked `Vec` access; the collector
  contains no raw pointers or `unsafe` blocks.
- Outside minor collection, every live `ObjId` resolves to an ordinary object,
  and no `Forwarded` variant is observable.
- After minor collection, no root, old object field, or old payload value points
  into the nursery ranges.
- A live object's non-empty side spans exist in the same generation as their
  owner and are owned by that object alone.
- Major collection operates after nursery evacuation and reuses mark-sweep for
  old slots; the current collector does not compact the old generation.
- `SOLVEIG_GC_STRESS=1` uses low fixed nursery and major thresholds.
  `SOLVEIG_GC_THRESHOLD=1` deliberately runs a minor collection at every
  allocation safe point; it does not force a whole-old-heap major collection
  after every promotion. Normal major collection starts after 32,768
  promotions and then uses an adaptive threshold based on the surviving old
  generation.

### Scheduling, blocking, and stopping

- `Interpreter::safe_point` is the sole reduction/GC suspension boundary.
  Calls, taken backward jumps, and allocations use it; interruption is
  observed when the suspended process resumes, not by a second bytecode poll.
- The saved state returned as `Yielded` is complete and rooted, and its PC is a
  resume offset. A yielded process returns to the back of the strict FIFO run
  queue.
- Deterministic scheduler decisions never depend on sleeps or wall-clock
  timing. Tests use virtual time, a fixed reduction budget, and exact FIFO
  order.
- A blocked process records one typed `BlockReason`. IO completion must match
  its `RequestId`; global wakeup must match its `GlobalIndex`; channel wakeup
  must match its `ChanId`; timer wakeup must match its `Deadline`.
  Interruption removes or cancels the corresponding scheduler registration
  before making the process runnable.
- An empty run queue waits only when an outstanding external completion can
  make progress. Otherwise the runtime reports deadlock instead of spinning.
- Main completion terminates program scheduling. Non-main failure is reported
  to stderr and does not fail main. Main failure terminates with its error.
- Host workers return owned plain Rust data only. All `Value` construction,
  heap allocation, wakeup, and process-state mutation occur on the scheduler
  thread.

### Required verification for runtime changes

Every runtime change runs the normal workspace build, tests, formatting, and
warnings-as-errors clippy gates, plus:

```text
SOLVEIG_GC_STRESS=1 cargo test -p solveig-vm
```

Concurrency tests use deterministic scheduler controls, gates, or channels;
they do not use sleeps or elapsed-time assertions. Any change to observable run
output needs task authorization and a recorded decision. A task adding a
mutable heap object must update the write-barrier inventory and add a stress
test that exercises an old-to-young edge.
