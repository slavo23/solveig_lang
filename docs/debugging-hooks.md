# Debugging hooks

Solveig does not yet implement a debugger or the Debug Adapter Protocol (DAP).
The VM does preserve the information and suspension boundaries that a future
debug adapter will need. This document defines those hooks and their current
limits so later runtime optimisations do not remove them accidentally.

## Source locations in bytecode

Every `Chunk` owns a `source_spans` table with exactly one entry per bytecode
instruction. The table is built in all profiles; it is not conditional on
debug assertions or a compiler flag. `Chunk::source_span(pc)` performs the
bytecode-offset lookup and returns `None` for an invalid or synthetic offset.

Checked expression spans are paired with their `FileId` by the driver and
carried through lowered IR and closure IR as `IRExpr::Located` and
`CCExpr::Located`. Bytecode lowering maintains the current enclosing location
while it emits instructions. A more specific child expression temporarily
replaces that location, so operands retain their own spans and an operation
retains the span of the expression it implements.

The table stores `Option<FileSpan>`. `None` is an explicit entry for synthetic
instructions or compiler-generated globals that have no honest source range;
it is not a missing table row. A future debugger must use the driver's retained
`SourceMap` to turn `FileId` and byte offsets into paths, lines, and columns.
It must not make the VM own source text or introduce a second position-mapping
implementation.

## Suspension and frame walking

Preemption, GC admission, future interruption delivery, and future debugger
stops share `Interpreter::safe_point`. Calls, taken backward jumps, and
allocation paths reach that boundary. When a reduction is exhausted, the
current operation first publishes all state into the `Process`; only then does
the interpreter return `StepOutcome::Yielded` to the scheduler.

A process returned as `Yielded` is therefore inspectable without executing it:

- its `pc` is the resume offset, normally the next instruction and, after a
  taken backward jump, the branch target;
- its value stack and frame base are complete;
- all heap values that could survive an allocation are in the root set;
- call and global-publication transitions have installed their continuations.

`Process::frames()` walks bytecode frames from the current frame outward to the
oldest caller. Each `ProcessFrame` records `chunk`, resume `pc`, and
`frame_base`. Both ordinary `CallerFrame` continuations and
`PublishGlobalThenResume` continuations contribute caller frames. `Halt`,
`PublishGlobalThenHalt`, and `IoBind` represent control state but do not invent
bytecode call frames.

The scheduler must be the owner of stopping and resuming processes. A debugger
may inspect frames only while the process is not being executed; it must not
hold references across a resume or allocation. In particular, `ObjId` and
`Value::Obj` are relocatable heap locations rather than stable debugger object
identities.

## Deliberate limitation: tail calls

`TailCallN` and `TailCallKnown` reuse the current bytecode frame. The active
callee and its nearest non-tail caller remain walkable, but the logical chain
of eliminated tail calls cannot be reconstructed. Consequently a future
debugger cannot show or “step out” through those elided logical frames without
new metadata or a debug execution mode. Any optimisation that elides another
kind of frame must be documented here with the same stepping consequence.

## What a future DAP implementation still needs

These hooks preserve feasibility; they are not a debugger API. A future DAP
task must still add:

- a scheduler-owned pause request checked by the existing safe-point path;
- breakpoint resolution from `FileSpan` to one or more chunk offsets, likely
  with an inverse index built from the permanent source tables;
- step-in, step-over, and step-out state expressed in terms of safe points and
  walkable frame depth, including an explicit tail-call policy;
- immutable stopped-process snapshots and stable protocol handles for frames,
  scopes, variables, and relocatable heap values;
- source-path and line/column conversion through the retained `SourceMap`;
- launch/attach lifecycle, stopped/continued/terminated events, and DAP
  transport in an outer integration layer.

I9 intentionally adds none of that protocol or breakpoint machinery. It only
keeps bytecode positions, frame boundaries, and safe-point state available for
the later implementation.
