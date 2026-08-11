//! Function, closure, constructor, and builtin invocation support.
//! Dispatch delegates call setup here when bytecode applies runtime values.

use super::*;
use crate::builtins::{self, VmCtx};
use crate::bytecode::{BytecodeProgram, ChunkKind};
use crate::heap::{BoundPrefix, HeapObject, PayloadRef, PayloadStore};
use crate::values::{Continuation, GlobalSlot, ObjId, Value};

impl Interpreter<'_> {
    #[inline(always)]
    pub(super) fn push(&mut self, val: Value) {
        self.stack.push(val);
    }

    #[inline(always)]
    pub(super) fn pop(&mut self) -> Value {
        // SAFETY: The bytecode emitter only emits pop-consuming instructions
        // after their operands have been pushed in the same stack frame.
        unsafe { self.stack.pop().unwrap_unchecked() }
    }

    #[inline(always)]
    pub(super) fn pop_int(&mut self) -> i64 {
        match self.pop() {
            Value::Int(n) => n,
            // SAFETY: Integer bytecode ops are emitted only after type checking
            // and lowering have proved both operands are Int.
            _ => unsafe { std::hint::unreachable_unchecked() },
        }
    }

    #[inline]
    pub(super) fn pop_payload(&mut self, n: usize) -> PayloadRef {
        self.heap
            .alloc_payload_from_stack_tail(&mut self.process.stack, n)
    }

    #[inline]
    pub(super) fn bool_value(&self, b: bool) -> Value {
        if b {
            self.bools.true_val
        } else {
            self.bools.false_val
        }
    }

    #[inline(always)]
    pub(super) fn load(&self, slot: u16) -> Value {
        // SAFETY: `ensure_slots` sizes the current frame before execution, and
        // bytecode local slots are allocated by ChunkBuilder for that frame.
        unsafe { *self.stack.get_unchecked(self.frame_base + slot as usize) }
    }

    #[inline(always)]
    pub(super) fn store(&mut self, slot: u16, val: Value) {
        let index = self.frame_base + slot as usize;
        // SAFETY: `ensure_slots` sizes the current frame before execution, and
        // bytecode local slots are allocated by ChunkBuilder for that frame.
        unsafe {
            *self.stack.get_unchecked_mut(index) = val;
        }
    }

    #[inline(always)]
    fn stack_value_at(&self, index: usize) -> Value {
        *self
            .stack
            .get(index)
            .expect("ICE: VM stack read index out of bounds")
    }

    #[inline(always)]
    fn set_stack_value_at(&mut self, index: usize, value: Value) {
        *self
            .stack
            .get_mut(index)
            .expect("ICE: VM stack write index out of bounds") = value;
    }

    pub(super) fn ensure_slots(&mut self, count: usize) {
        let needed = self.frame_base + count;
        if self.stack.len() < needed {
            self.stack.resize(needed, Value::Unit);
        }
    }

    #[inline]
    pub(super) fn push_caller_continuation(&mut self) {
        let continuation = Continuation::CallerFrame {
            chunk: self.chunk,
            pc: self.pc,
            frame_base: self.frame_base,
        };
        self.continuations.push(continuation);
    }

    pub(super) fn start_global_eval(
        &mut self,
        program: &BytecodeProgram,
        index: usize,
        continuation: Continuation,
    ) -> Result<Option<Value>, RuntimeError> {
        let Some(global_slot) = self.globals.get(index) else {
            solveig_base::ice!("global index {index} out of bounds");
        };
        let thunk_chunk = match global_slot {
            GlobalSlot::Evaluated(val) => return Ok(Some(*val)),
            GlobalSlot::Evaluating => {
                let name = program
                    .globals
                    .get(index)
                    .map_or("<unknown>", |global| global.name.as_str());
                return Err(RuntimeError::cyclic_global(name));
            }
            GlobalSlot::Thunk { chunk } => *chunk,
        };

        *self
            .globals
            .get_mut(index)
            .expect("ICE: global slot index was checked above") = GlobalSlot::Evaluating;
        self.continuations.push(continuation);
        self.enter_thunk(program, thunk_chunk);
        Ok(None)
    }

    pub(super) fn push_global_or_start_eval(
        &mut self,
        program: &BytecodeProgram,
        index: usize,
    ) -> Result<(), RuntimeError> {
        let continuation = Continuation::PublishGlobalThenResume {
            global_index: index,
            chunk: self.chunk,
            pc: self.pc,
            frame_base: self.frame_base,
        };

        if let Some(val) = self.start_global_eval(program, index, continuation)? {
            self.push(val);
        }

        Ok(())
    }

    pub(super) fn enter_thunk(&mut self, program: &BytecodeProgram, chunk_id: usize) {
        let meta = program.chunk(chunk_id);
        self.chunk = chunk_id;
        self.pc = 0;
        self.frame_base = self.stack.len();
        let frame_end = self.frame_base + meta.slot_count as usize;
        self.stack.resize(frame_end, Value::Unit);
    }

    #[inline(always)]
    pub(super) fn begin_frame(
        &mut self,
        program: &BytecodeProgram,
        mode: InvokeMode,
        chunk_id: usize,
        arg_count: usize,
    ) {
        let slot_count = program.chunk(chunk_id).slot_count as usize;
        let args_start = self.stack.len() - arg_count;

        match mode {
            InvokeMode::Call | InvokeMode::HaltCall => {
                if matches!(mode, InvokeMode::HaltCall) {
                    self.continuations.push(Continuation::Halt);
                } else {
                    self.push_caller_continuation();
                }
                self.chunk = chunk_id;
                self.pc = 0;
                self.frame_base = args_start;
            }
            InvokeMode::TailCall => {
                if args_start != self.frame_base {
                    for i in 0..arg_count {
                        let value = self.stack_value_at(args_start + i);
                        self.set_stack_value_at(self.frame_base + i, value);
                    }
                }

                let frame_end = self.frame_base + arg_count;
                self.stack.truncate(frame_end);
                self.chunk = chunk_id;
                self.pc = 0;
            }
        }

        self.ensure_slots(slot_count);
    }

    #[inline(always)]
    pub(super) fn prepend_bound_prefix(&mut self, bound: BoundPrefix, arg_count: usize) {
        if bound.is_empty() {
            return;
        }

        let prefix_len = bound.total_len();
        let Some(owner) = bound.owner else {
            solveig_base::ice!("non-empty bound prefix has no owning heap object");
        };

        for i in (0..arg_count).rev() {
            let value = self.stack_value_at(self.frame_base + i);
            self.set_stack_value_at(self.frame_base + prefix_len + i, value);
        }

        let frame_base = self.frame_base;
        let (heap, stack) = (&*self.heap, &mut self.process.stack);

        if !bound.captures.is_empty() {
            heap.copy_payload_to_stack(owner, bound.captures, stack, frame_base);
        }

        if !bound.applied_args.is_empty() {
            heap.copy_payload_to_stack(
                owner,
                bound.applied_args,
                stack,
                frame_base + bound.captures.len(),
            );
        }
    }

    #[inline(always)]
    pub(super) fn enter_function(
        &mut self,
        program: &BytecodeProgram,
        mode: InvokeMode,
        chunk_id: usize,
        bound: BoundPrefix,
        arg_count: usize,
    ) {
        self.begin_frame(program, mode, chunk_id, arg_count);
        self.prepend_bound_prefix(bound, arg_count);
    }

    #[inline(always)]
    pub(super) fn enter_known_function(
        &mut self,
        program: &BytecodeProgram,
        mode: InvokeMode,
        chunk_id: usize,
        arg_count: usize,
    ) {
        self.enter_function(program, mode, chunk_id, BoundPrefix::EMPTY, arg_count);
    }

    pub(super) fn invoke_known_function(
        &mut self,
        program: &BytecodeProgram,
        mode: InvokeMode,
        chunk_id: usize,
        arg_count: usize,
    ) -> Option<Value> {
        let (capture_count, arity) = chunk_function_shape(program, chunk_id);

        if capture_count != 0 {
            solveig_base::ice!(
                "known call to captured function chunk {chunk_id} (captures {capture_count}, arity {arity})"
            );
        }

        if arg_count == arity {
            self.enter_known_function(program, mode, chunk_id, arg_count);
            None
        } else if arg_count < arity {
            if arg_count == 0 {
                let chunk = u32::try_from(chunk_id).unwrap_or_else(|_| {
                    solveig_base::ice!("chunk id {chunk_id} does not fit in u32")
                });
                let arity = u16::try_from(arity)
                    .unwrap_or_else(|_| solveig_base::ice!("arity {arity} does not fit in u16"));

                Some(Value::FunctionRef { chunk, arity })
            } else {
                self.maybe_gc();
                let applied_args = self.pop_payload(arg_count);

                Some(self.alloc_heap(HeapObject::PartialApp {
                    chunk: chunk_id,
                    captures: PayloadStore::empty(),
                    applied_args,
                }))
            }
        } else {
            solveig_base::ice!(
                "over-application of known function chunk {chunk_id} (arity {arity}, got {arg_count})"
            );
        }
    }

    #[inline(always)]
    pub(super) fn take_callee_under_args(&mut self, arg_count: usize) -> Value {
        let callee_pos = self.stack.len() - arg_count - 1;
        let callee = self.stack_value_at(callee_pos);

        for i in callee_pos..self.stack.len() - 1 {
            let value = self.stack_value_at(i + 1);
            self.set_stack_value_at(i, value);
        }
        let Some(_removed_duplicate) = self.stack.pop() else {
            solveig_base::ice!("callee removal underflowed the VM stack");
        };

        callee
    }

    pub(super) fn invoke_value(
        &mut self,
        program: &BytecodeProgram,
        mode: InvokeMode,
        callee: Value,
        arg_count: usize,
    ) -> Option<Value> {
        match callee {
            Value::CtorFn { tag, arity } => {
                self.maybe_gc();
                Some(self.apply_constructor(None, tag, arity, PayloadStore::empty(), arg_count))
            }

            Value::FunctionRef { chunk, arity } => {
                let chunk_id = chunk as usize;
                let arity = arity as usize;

                if arg_count == arity {
                    self.enter_known_function(program, mode, chunk_id, arg_count);
                    None
                } else if arg_count < arity {
                    self.maybe_gc();
                    let applied_args = self.pop_payload(arg_count);

                    Some(self.alloc_heap(HeapObject::PartialApp {
                        chunk: chunk_id,
                        captures: PayloadStore::empty(),
                        applied_args,
                    }))
                } else {
                    solveig_base::ice!(
                        "over-application of function reference (arity {arity}, got {arg_count})"
                    );
                }
            }

            Value::Obj(id) => {
                let checkpoint = self.temporary_roots.len();
                self.temporary_roots.push(Value::Obj(id));
                self.maybe_gc();
                let Some(rooted_callee) = self.temporary_roots.get(checkpoint).copied() else {
                    solveig_base::ice!("rooted heap callee disappeared");
                };
                let Value::Obj(id) = rooted_callee else {
                    solveig_base::ice!("heap callee root changed value kind during collection");
                };
                let result = self.invoke_heap_object(program, mode, id, arg_count);
                self.temporary_roots.truncate(checkpoint);
                result
            }

            _ => solveig_base::ice!(
                "cannot {}call {:?}",
                if matches!(mode, InvokeMode::TailCall) {
                    "tail-"
                } else {
                    ""
                },
                callee
            ),
        }
    }

    pub(super) fn invoke_heap_object(
        &mut self,
        program: &BytecodeProgram,
        mode: InvokeMode,
        id: ObjId,
        arg_count: usize,
    ) -> Option<Value> {
        match self.heap.get(id) {
            HeapObject::Closure { chunk, captures } => {
                let chunk = *chunk;
                let captures = *captures;
                let arity = chunk_arity(program, chunk);

                if arg_count == arity {
                    self.enter_function(
                        program,
                        mode,
                        chunk,
                        BoundPrefix {
                            owner: Some(id),
                            captures,
                            applied_args: PayloadStore::empty(),
                        },
                        arg_count,
                    );
                    None
                } else if arg_count < arity {
                    let applied_args = self.pop_payload(arg_count);
                    let captures = self.heap.clone_payload(id, captures);

                    Some(self.alloc_heap(HeapObject::PartialApp {
                        chunk,
                        captures,
                        applied_args,
                    }))
                } else {
                    solveig_base::ice!(
                        "over-application of closure (arity {arity}, got {arg_count})"
                    );
                }
            }

            HeapObject::PartialApp {
                chunk,
                captures,
                applied_args,
            } => {
                let chunk = *chunk;
                let captures = *captures;
                let applied_args = *applied_args;
                let arity = chunk_arity(program, chunk);
                let total = applied_args.len() + arg_count;

                if total == arity {
                    self.enter_function(
                        program,
                        mode,
                        chunk,
                        BoundPrefix {
                            owner: Some(id),
                            captures,
                            applied_args,
                        },
                        arg_count,
                    );
                    None
                } else if total < arity {
                    let captures = self.heap.clone_payload(id, captures);
                    let applied_args = self.heap.alloc_concat_payload_and_stack_tail(
                        Some(id),
                        applied_args,
                        &mut self.process.stack,
                        arg_count,
                    );

                    Some(self.alloc_heap(HeapObject::PartialApp {
                        chunk,
                        captures,
                        applied_args,
                    }))
                } else {
                    solveig_base::ice!("over-application of partial (arity {arity}, got {total})");
                }
            }

            HeapObject::PartialCtor {
                tag,
                arity,
                applied_args,
            } => Some(self.apply_constructor(Some(id), *tag, *arity, *applied_args, arg_count)),

            HeapObject::Data0 { tag }
            | HeapObject::Data1 { tag, .. }
            | HeapObject::Data2 { tag, .. }
            | HeapObject::DataN { tag, .. } => {
                solveig_base::ice!("cannot apply saturated constructor tag {tag}");
            }

            HeapObject::Str { .. } => {
                solveig_base::ice!("heap string reached callable-object invocation");
            }

            HeapObject::IoAction(_) => {
                solveig_base::ice!("IO action reached callable-object invocation");
            }
            HeapObject::Forwarded(_) => {
                solveig_base::ice!("forwarding entry reached callable-object invocation");
            }
        }
    }

    pub(super) fn apply_constructor(
        &mut self,
        owner: Option<ObjId>,
        tag: u16,
        arity: u16,
        existing: PayloadRef,
        n_new: usize,
    ) -> Value {
        let total = existing.len() + n_new;

        if total == arity as usize {
            if existing.is_empty() {
                return match n_new {
                    0 => self.alloc_heap(HeapObject::Data0 { tag }),
                    1 => {
                        let field = self.pop();
                        self.alloc_heap(HeapObject::Data1 { tag, field })
                    }
                    2 => {
                        let field1 = self.pop();
                        let field0 = self.pop();
                        self.alloc_heap(HeapObject::Data2 {
                            tag,
                            field0,
                            field1,
                        })
                    }
                    _ => {
                        let fields = self.pop_payload(n_new);
                        self.alloc_heap(HeapObject::DataN { tag, fields })
                    }
                };
            }

            let fields = self.heap.alloc_concat_payload_and_stack_tail(
                owner,
                existing,
                &mut self.process.stack,
                n_new,
            );
            self.alloc_heap(HeapObject::DataN { tag, fields })
        } else if total < arity as usize {
            let applied_args = self.heap.alloc_concat_payload_and_stack_tail(
                owner,
                existing,
                &mut self.process.stack,
                n_new,
            );

            self.alloc_heap(HeapObject::PartialCtor {
                tag,
                arity,
                applied_args,
            })
        } else {
            solveig_base::ice!("over-applied constructor tag {tag}");
        }
    }

    pub(super) fn call_builtin(
        &mut self,
        id: solveig_ast::ast::BuiltinId,
        arg_count: usize,
    ) -> Result<Value, RuntimeError> {
        let Some(builtin) = builtins::lookup(id) else {
            return Err(RuntimeError::builtin(format!(
                "unknown builtin id {}",
                id.0
            )));
        };

        if arg_count != builtin.arity {
            return Err(RuntimeError::builtin(format!(
                "builtin `{}` expected {} arguments, got {}",
                builtin.name, builtin.arity, arg_count
            )));
        }

        self.maybe_gc();

        let Some(args_start) = self.stack.len().checked_sub(arg_count) else {
            solveig_base::ice!("builtin call stack underflow");
        };
        let args = self
            .stack
            .get(args_start..)
            .expect("ICE: builtin argument slice out of bounds")
            .to_vec();
        let mut ctx = VmCtx::new(self);
        let result = (builtin.run)(&mut ctx, &args)?;
        self.stack.truncate(args_start);

        Ok(result)
    }

    pub(super) fn call_value_for_result(
        &mut self,
        program: &BytecodeProgram,
        callee: Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        for arg in args {
            self.push(*arg);
        }

        match self.invoke_value(program, InvokeMode::HaltCall, callee, args.len()) {
            Some(value) => Ok(value),
            None => self.dispatch(program),
        }
    }

    pub(super) fn resume_after_return(&mut self, result: Value) -> Option<Value> {
        let frame_base = self.frame_base;
        self.stack.truncate(frame_base);

        let Some(continuation) = self.continuations.pop() else {
            solveig_base::ice!("return with empty continuation stack");
        };

        match continuation {
            Continuation::Halt => Some(result),

            Continuation::CallerFrame {
                chunk,
                pc,
                frame_base,
            } => {
                self.chunk = chunk;
                self.pc = pc;
                self.frame_base = frame_base;
                self.push(result);
                None
            }

            Continuation::PublishGlobalThenHalt { global_index } => {
                self.publish_global(global_index, result);
                Some(result)
            }

            Continuation::PublishGlobalThenResume {
                global_index,
                chunk,
                pc,
                frame_base,
            } => {
                self.publish_global(global_index, result);
                self.chunk = chunk;
                self.pc = pc;
                self.frame_base = frame_base;
                self.push(result);
                None
            }

            Continuation::IoBind(_) => {
                solveig_base::ice!("IO continuation reached bytecode return handling")
            }
        }
    }

    /// The sole mutable publication site in the I4 write-barrier inventory.
    /// Globals are scanned as roots on every minor collection, so publication
    /// needs no remembered-set insertion while remaining collector-visible.
    fn publish_global(&mut self, global_index: usize, result: Value) {
        let Some(global_slot) = self.globals.get_mut(global_index) else {
            solveig_base::ice!("global publish index {global_index} out of bounds");
        };
        *global_slot = GlobalSlot::Evaluated(result);
    }
}

fn chunk_function_shape(program: &BytecodeProgram, chunk_id: usize) -> (usize, usize) {
    match program.chunk(chunk_id).kind {
        ChunkKind::Function {
            capture_count,
            arity,
        } => (capture_count as usize, arity as usize),
        ChunkKind::GlobalThunk => solveig_base::ice!("thunk has no arity"),
    }
}

fn chunk_arity(program: &BytecodeProgram, chunk_id: usize) -> usize {
    let (_, arity) = chunk_function_shape(program, chunk_id);
    arity
}
