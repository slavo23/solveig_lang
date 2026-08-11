# Solveig VM

This document records the runtime contracts that later VM work must preserve.
Task I3 establishes the object-layout and relocation contract before task I4
changes the collector. Task I10 will expand this into the complete VM guide.

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
objects do not move during I4: major collection remains mark-sweep, without
compaction.

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

`HeapObject` gains a `Forwarded(ObjId)` variant. A forwarding entry is valid
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
3. Scan the remembered set described below. It is empty for today's immutable
   heap objects, but the collector boundary must support it before channels add
   mutable heap state.
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
Old-generation ids happen not to move in I4, but callers must not specialize
their behavior around that fact. Equality and hashing of `ObjId` are meaningful
only inside a non-allocating epoch. Long-lived runtime identity uses its own
typed ids, such as `ProcessId`; it never uses an object location.

Collector entry is an allocation safe point. Before entering it, interpreter
temporaries that contain `Value`s must have been published into the active
`Process` or its `temporary_roots`. The one `Roots` implementation then rewrites
all of these locations:

- the active process and every inactive process: value stack, binding buffer,
  temporary roots, finished result, current IO action, and `IoBind`
  continuations;
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

The I3/I4 inventory is:

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
- Phase J's channel state is the first planned mutable heap object. J1 must add
  its mutation operations here and insert an old channel into the remembered
  set whenever it receives a nursery value.

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

The process continuation stack is the ownership record: each live
`PublishGlobalThen*` frame identifies one global the process must either
publish or fail. There is no second per-process blackhole stack.

## Safe points and preemption

`Interpreter::safe_point` is the only runtime boundary that consumes a
process reduction and admits GC. Calls, taken backward jumps, and allocation
paths all reach this function. Interruption delivery and debugger stops must
extend this function in their later phases instead of adding another polling
path.

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

## Scheduler, `spawn`, and program lifetime

Solveig runs lightweight processes on one OS thread. The scheduler removes the
front runnable process, runs its IO machine until it yields, blocks, finishes,
or fails, and then re-queues, parks, or retires that process. A spawned process
therefore shares the VM heap and global table but owns its instruction state,
stacks, continuation frames, status, and reduction budget. This is the
lightweight-thread model described in Peyton Jones, Gordon, and Finne,
“Concurrent Haskell” (POPL 1996), with the explicit scheduler return reasons
described by the AOSA GHC runtime chapter.

`spawn : IO () -> IO ThreadId` constructs an inert IO action. Performing that
action allocates a process, puts its child action in the process root set, and
appends it to the FIFO run queue. `ThreadId` is an opaque primitive type: its
runtime payload identifies the process arena entry, but the language exposes no
numeric operations or representation conversion for it.

Program lifetime is owned by the main process:

- When main finishes successfully, the scheduler returns its result and the
  runtime is dropped. Any other runnable or blocked Solveig processes are
  killed; their pending IO completions are not resumed. The scoped host worker
  threads are joined before a library-backed execution call returns, so an
  already-running host call is allowed to finish without touching VM state.
- When main fails, that `RuntimeError` terminates the program.
- When a non-main process fails, the scheduler writes
  `thread <id> failed: <error>` to stderr, retires that process, and continues
  running the program. The failure does not propagate to main.

The ordering contract remains deterministic FIFO. `SOLVEIG_REDUCTIONS` fixes
the quantum used by every process, and `SOLVEIG_SCHED_DETERMINISTIC=1` pins the
policy explicitly for tests. Scheduling never uses sleeps or elapsed time.

## Blocking IO workers

Blocking host IO does not run on the scheduler thread. `Runtime` owns an
`IoSubsystem` with four scoped OS workers, a typed request table, and one
multi-producer/single-consumer completion channel. Performing `printLine`,
`printStr`, `readLine`, `readFile`, `writeFile`, or `appendFile` follows this
state transition:

1. The scheduler thread resolves every VM operand to owned Rust data and
   submits an `IoRequest` associated with the current `ProcessId`.
2. The process records `Blocked(Io(RequestId))` and leaves the run queue.
3. A worker performs the host call and returns only `String`, unit, or a
   contextual `io::Error`. Workers never hold `Value`, `ObjId`, heap roots, or
   any other VM reference.
4. The scheduler drains completions, verifies that request and block reason
   match, stores the plain result on the process, and returns it to the FIFO
   run queue. That process converts text into a heap string at its next
   allocation safe point on the scheduler thread.

Completed request-table slots are reused only after their completion has been
consumed, so a `RequestId` identifies at most one outstanding operation. The
table contains the process association rather than encoding it in the id.

Terminal reads and writes share worker zero and therefore retain submission
order on the caller-provided streams. File requests are distributed
round-robin across the other three workers and may finish in a different order;
each completion wakes only its requesting process. Stream references are
borrowed by scoped workers and protected by `Mutex` because multiple worker
threads exist, but the mutex does not protect compiler or VM heap state.

Every scheduler iteration first drains ready completions. If the run queue is
empty while IO requests remain, it blocks on the completion receiver rather
than polling or sleeping. If the queue is empty and no request can complete,
execution fails with `all threads are blocked forever`. This is the external
completion/deadlock distinction used by GHC's scheduler, described in the AOSA
GHC runtime chapter. Standard-library per-worker `mpsc` queues express this
I8 topology; Phase J's requirement to wait on either a completion or a timer
deadline is the point at which a selecting channel may become necessary.

## Collector invariants for I4

- Allocation and collection use indices and checked `Vec` access; the collector
  contains no raw pointers or `unsafe` blocks.
- Outside minor collection, every live `ObjId` resolves to an ordinary object,
  and no `Forwarded` variant is observable.
- After minor collection, no root, old object field, or old payload value points
  into the nursery ranges.
- A live object's non-empty side spans exist in the same generation as their
  owner and are owned by that object alone.
- Major collection operates after nursery evacuation and reuses mark-sweep for
  old slots; I4 does not compact the old generation.
- `SOLVEIG_GC_STRESS=1` uses low fixed nursery and major thresholds.
  `SOLVEIG_GC_THRESHOLD=1` deliberately runs a minor collection at every
  allocation safe point; it does not force a whole-old-heap major collection
  after every promotion. Normal major collection starts after 32,768
  promotions and then uses an adaptive threshold based on the surviving old
  generation.

These invariants preserve precise rooting and deterministic behavior while
containing all representation knowledge inside `solveig-vm`'s heap and GC
modules.
