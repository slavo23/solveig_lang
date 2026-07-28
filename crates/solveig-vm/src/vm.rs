use solveig_ast::ast::{CtorId, CtorRuntimeInfo, Literal, PRIM_TYS};

use super::bytecode::{BytecodeProgram, ChunkKind, Instr, MatchArm, lower_to_bytecode};
use super::heap::{BoundPrefix, Heap, HeapObject, IoAction, PayloadRef, PayloadStore};
use super::values::{BoolCache, Continuation, GlobalSlot, NullaryCtorCache, ObjId, Value};
use crate::builtins::{self, VmCtx};
use crate::display::DisplayValue;
use solveig_lower::LowerPattern;
use solveig_lower::closure_ir::CCProgram;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, Cursor, Write};
use std::path::{Path, PathBuf};

struct Vm {
    chunk: usize,
    pc: usize,
    frame_base: usize,

    stack: Vec<Value>,
    continuations: Vec<Continuation>,
    binding_buffer: Vec<Value>,

    heap: Heap,
    globals: Vec<GlobalSlot>,
    nullary_ctors: NullaryCtorCache,
    bools: BoolCache,
}

#[inline]
fn mark_bit(mark_bits: &mut [bool], id: ObjId, worklist: &mut Vec<ObjId>) {
    let idx = id.0 as usize;
    if idx < mark_bits.len() && !mark_bits[idx] {
        mark_bits[idx] = true;
        worklist.push(id);
    }
}

#[inline]
fn mark_payload(
    payloads: &PayloadStore,
    mark_bits: &mut [bool],
    span: &PayloadRef,
    worklist: &mut Vec<ObjId>,
) {
    for &v in payloads.as_slice(*span) {
        if let Value::Obj(id) = v {
            mark_bit(mark_bits, id, worklist);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InvokeMode {
    Call,
    HaltCall,
    TailCall,
}

struct IoRuntime<'a> {
    stdin: &'a mut dyn BufRead,
    stdout: &'a mut dyn Write,
    args: &'a [String],
    cwd: &'a Path,
    line_buffer: String,
}

impl<'a> IoRuntime<'a> {
    fn new(
        stdin: &'a mut dyn BufRead,
        stdout: &'a mut dyn Write,
        args: &'a [String],
        cwd: &'a Path,
    ) -> Self {
        Self {
            stdin,
            stdout,
            args,
            cwd,
            line_buffer: String::new(),
        }
    }

    fn read_line(&mut self) -> Result<String, RuntimeError> {
        self.line_buffer.clear();
        let bytes = self
            .stdin
            .read_line(&mut self.line_buffer)
            .map_err(|err| RuntimeError::builtin(format!("readLine failed: {err}")))?;
        if bytes == 0 {
            return Ok(String::new());
        }

        if self.line_buffer.ends_with('\n') {
            self.line_buffer.pop();
            if self.line_buffer.ends_with('\r') {
                self.line_buffer.pop();
            }
        }

        Ok(self.line_buffer.clone())
    }

    fn write_str(&mut self, text: &str) -> Result<(), RuntimeError> {
        self.stdout
            .write_all(text.as_bytes())
            .and_then(|()| self.stdout.flush())
            .map_err(|err| RuntimeError::builtin(format!("IO write failed: {err}")))
    }

    fn resolve_path(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        }
    }
}

impl Vm {
    fn new(program: &BytecodeProgram) -> Self {
        let max_tag = program
            .constructor_arities
            .iter()
            .map(|info| info.tag as usize)
            .max()
            .unwrap_or(0);

        let mut vm = Vm {
            chunk: 0,
            pc: 0,
            frame_base: 0,
            stack: Vec::with_capacity(4096),
            continuations: Vec::with_capacity(512),
            binding_buffer: Vec::with_capacity(8),
            heap: Heap::default(),
            globals: program
                .globals
                .iter()
                .map(|g| GlobalSlot::Thunk { chunk: g.chunk })
                .collect(),
            nullary_ctors: NullaryCtorCache::new(max_tag),
            bools: BoolCache {
                true_val: Value::Unit,
                false_val: Value::Unit,
            },
        };

        vm.init_nullary(&program.constructor_arities);
        vm
    }

    fn init_nullary(&mut self, arities: &[CtorRuntimeInfo]) {
        for info in arities {
            if info.arity == 0 {
                let val = self.alloc_heap(HeapObject::Data0 { tag: info.tag });
                self.nullary_ctors.set(info.tag, val);
            }
        }

        let true_tag = find_ctor_tag(arities, PRIM_TYS.true_ctor);
        let false_tag = find_ctor_tag(arities, PRIM_TYS.false_ctor);
        self.bools.true_val = self.nullary_ctors.get(true_tag);
        self.bools.false_val = self.nullary_ctors.get(false_tag);
    }

    #[inline]
    fn alloc_heap(&mut self, obj: HeapObject) -> Value {
        self.heap.alloc(obj)
    }

    fn maybe_gc(&mut self) {
        if self.heap.allocations_since_gc >= self.heap.next_gc_at {
            self.collect_garbage();
        }
    }

    fn collect_garbage(&mut self) {
        let mut worklist: Vec<ObjId> = Vec::new();

        for i in 0..self.stack.len() {
            if let Value::Obj(id) = self.stack[i] {
                mark_bit(&mut self.heap.mark_bits, id, &mut worklist);
            }
        }

        for &val in &self.binding_buffer {
            if let Value::Obj(id) = val {
                mark_bit(&mut self.heap.mark_bits, id, &mut worklist);
            }
        }

        for i in 0..self.globals.len() {
            if let GlobalSlot::Evaluated(Value::Obj(id)) = self.globals[i] {
                mark_bit(&mut self.heap.mark_bits, id, &mut worklist);
            }
        }

        for slot in &self.nullary_ctors.tags {
            if let Some(Value::Obj(id)) = slot {
                mark_bit(&mut self.heap.mark_bits, *id, &mut worklist);
            }
        }

        if let Value::Obj(id) = self.bools.true_val {
            mark_bit(&mut self.heap.mark_bits, id, &mut worklist);
        }
        if let Value::Obj(id) = self.bools.false_val {
            mark_bit(&mut self.heap.mark_bits, id, &mut worklist);
        }

        while let Some(id) = worklist.pop() {
            enum Children {
                None,
                One(PayloadRef),
                Two(PayloadRef, PayloadRef),
            }

            let children = match self.heap.objects[id.0 as usize].as_ref() {
                Some(HeapObject::Closure { captures, .. }) => Children::One(*captures),
                Some(HeapObject::PartialApp {
                    captures,
                    applied_args,
                    ..
                }) => Children::Two(*captures, *applied_args),
                Some(HeapObject::PartialCtor { applied_args, .. }) => Children::One(*applied_args),
                Some(HeapObject::DataN { fields, .. }) => Children::One(*fields),
                Some(HeapObject::Data0 { .. }) => Children::None,
                Some(HeapObject::Data1 { field, .. }) => {
                    if let Value::Obj(child) = field {
                        mark_bit(&mut self.heap.mark_bits, *child, &mut worklist);
                    }
                    Children::None
                }
                Some(HeapObject::Data2 { field0, field1, .. }) => {
                    if let Value::Obj(child) = field0 {
                        mark_bit(&mut self.heap.mark_bits, *child, &mut worklist);
                    }
                    if let Value::Obj(child) = field1 {
                        mark_bit(&mut self.heap.mark_bits, *child, &mut worklist);
                    }
                    Children::None
                }
                Some(HeapObject::Str { .. }) => Children::None,
                Some(HeapObject::IoAction(action)) => {
                    match action {
                        IoAction::Pure(value) | IoAction::Print { text: value, .. } => {
                            if let Value::Obj(child) = value {
                                mark_bit(&mut self.heap.mark_bits, *child, &mut worklist);
                            }
                        }
                        IoAction::Bind {
                            action,
                            continuation,
                        } => {
                            if let Value::Obj(child) = action {
                                mark_bit(&mut self.heap.mark_bits, *child, &mut worklist);
                            }
                            if let Value::Obj(child) = continuation {
                                mark_bit(&mut self.heap.mark_bits, *child, &mut worklist);
                            }
                        }
                        IoAction::ReadLine | IoAction::GetArgs => {}
                        IoAction::ReadFile { path } => {
                            if let Value::Obj(child) = path {
                                mark_bit(&mut self.heap.mark_bits, *child, &mut worklist);
                            }
                        }
                        IoAction::WriteFile { path, contents, .. } => {
                            if let Value::Obj(child) = path {
                                mark_bit(&mut self.heap.mark_bits, *child, &mut worklist);
                            }
                            if let Value::Obj(child) = contents {
                                mark_bit(&mut self.heap.mark_bits, *child, &mut worklist);
                            }
                        }
                    }
                    Children::None
                }
                None => Children::None,
            };

            let payloads = &self.heap.payloads;
            let mark_bits = &mut self.heap.mark_bits;

            match children {
                Children::None => {}
                Children::One(span) => {
                    mark_payload(payloads, mark_bits, &span, &mut worklist);
                }
                Children::Two(a, b) => {
                    mark_payload(payloads, mark_bits, &a, &mut worklist);
                    mark_payload(payloads, mark_bits, &b, &mut worklist);
                }
            }
        }

        self.heap.payloads.bucket1.clear();
        self.heap.payloads.bucket2.clear();
        self.heap.payloads.bucket3.clear();
        self.heap.payloads.free.clear();
        self.heap.free_list.clear();

        let mut live = 0usize;
        for idx in 0..self.heap.objects.len() {
            if self.heap.objects[idx].is_some() {
                if self.heap.mark_bits[idx] {
                    self.heap.mark_bits[idx] = false;
                    live += 1;
                } else {
                    let dead = self.heap.objects[idx].take().unwrap();
                    self.heap.free_object_payloads(&dead);
                    self.heap.free_list.push(idx as u32);
                }
            }
        }

        self.heap.allocations_since_gc = 0;
        self.heap.next_gc_at = live.max(65_536) * 4;
    }

    #[inline(always)]
    fn push(&mut self, val: Value) {
        self.stack.push(val);
    }

    #[inline(always)]
    fn pop(&mut self) -> Value {
        unsafe { self.stack.pop().unwrap_unchecked() }
    }

    #[inline(always)]
    fn pop_int(&mut self) -> i64 {
        match self.pop() {
            Value::Int(n) => n,
            _ => unsafe { std::hint::unreachable_unchecked() },
        }
    }

    #[inline]
    fn pop_payload(&mut self, n: usize) -> PayloadRef {
        self.heap.alloc_payload_from_stack_tail(&mut self.stack, n)
    }

    #[inline]
    fn bool_value(&self, b: bool) -> Value {
        if b {
            self.bools.true_val
        } else {
            self.bools.false_val
        }
    }

    #[inline(always)]
    fn load(&self, slot: u16) -> Value {
        unsafe { *self.stack.get_unchecked(self.frame_base + slot as usize) }
    }

    #[inline(always)]
    fn store(&mut self, slot: u16, val: Value) {
        unsafe {
            *self
                .stack
                .get_unchecked_mut(self.frame_base + slot as usize) = val;
        }
    }

    fn ensure_slots(&mut self, count: usize) {
        let needed = self.frame_base + count;
        if self.stack.len() < needed {
            self.stack.resize(needed, Value::Unit);
        }
    }

    #[inline]
    fn push_caller_continuation(&mut self) {
        self.continuations.push(Continuation::CallerFrame {
            chunk: self.chunk,
            pc: self.pc,
            frame_base: self.frame_base,
        });
    }

    fn start_global_eval(
        &mut self,
        program: &BytecodeProgram,
        index: usize,
        continuation: Continuation,
    ) -> Result<Option<Value>, RuntimeError> {
        let thunk_chunk = match &self.globals[index] {
            GlobalSlot::Evaluated(val) => return Ok(Some(*val)),
            GlobalSlot::Evaluating => {
                let name = program
                    .globals
                    .get(index)
                    .map(|global| global.name.as_str())
                    .unwrap_or("<unknown>");
                return Err(RuntimeError::cyclic_global(name));
            }
            GlobalSlot::Thunk { chunk } => *chunk,
        };

        self.globals[index] = GlobalSlot::Evaluating;
        self.continuations.push(continuation);
        self.enter_thunk(program, thunk_chunk);
        Ok(None)
    }

    fn eval_global(
        &mut self,
        program: &BytecodeProgram,
        index: usize,
    ) -> Result<Value, RuntimeError> {
        match self.start_global_eval(
            program,
            index,
            Continuation::PublishGlobalThenHalt {
                global_index: index,
            },
        )? {
            Some(val) => Ok(val),
            None => self.dispatch(program),
        }
    }

    fn push_global_or_start_eval(
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

    fn enter_thunk(&mut self, program: &BytecodeProgram, chunk_id: usize) {
        let meta = program.chunk(chunk_id);
        self.chunk = chunk_id;
        self.pc = 0;
        self.frame_base = self.stack.len();
        self.stack
            .resize(self.frame_base + meta.slot_count as usize, Value::Unit);
    }

    #[inline(always)]
    fn begin_frame(
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
                        self.stack[self.frame_base + i] = self.stack[args_start + i];
                    }
                }

                self.stack.truncate(self.frame_base + arg_count);
                self.chunk = chunk_id;
                self.pc = 0;
            }
        }

        self.ensure_slots(slot_count);
    }

    #[inline(always)]
    fn prepend_bound_prefix(&mut self, bound: BoundPrefix, arg_count: usize) {
        if bound.is_empty() {
            return;
        }

        let prefix_len = bound.total_len();

        for i in (0..arg_count).rev() {
            self.stack[self.frame_base + prefix_len + i] = self.stack[self.frame_base + i];
        }

        let frame_base = self.frame_base;
        let (heap, stack) = (&self.heap, &mut self.stack);

        if !bound.captures.is_empty() {
            heap.copy_payload_to_stack(bound.captures, stack, frame_base);
        }

        if !bound.applied_args.is_empty() {
            heap.copy_payload_to_stack(
                bound.applied_args,
                stack,
                frame_base + bound.captures.len(),
            );
        }
    }

    #[inline(always)]
    fn enter_function(
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
    fn enter_known_function(
        &mut self,
        program: &BytecodeProgram,
        mode: InvokeMode,
        chunk_id: usize,
        arg_count: usize,
    ) {
        self.enter_function(program, mode, chunk_id, BoundPrefix::EMPTY, arg_count);
    }

    fn invoke_known_function(
        &mut self,
        program: &BytecodeProgram,
        mode: InvokeMode,
        chunk_id: usize,
        arg_count: usize,
    ) -> Option<Value> {
        let (capture_count, arity) = chunk_function_shape(program, chunk_id);

        if capture_count != 0 {
            panic!(
                "ICE: known call to captured function chunk {} (captures {}, arity {})",
                chunk_id, capture_count, arity
            );
        }

        if arg_count == arity {
            self.enter_known_function(program, mode, chunk_id, arg_count);
            None
        } else if arg_count < arity {
            if arg_count == 0 {
                let chunk = u32::try_from(chunk_id)
                    .unwrap_or_else(|_| panic!("ICE: chunk id {} does not fit in u32", chunk_id));
                let arity = u16::try_from(arity)
                    .unwrap_or_else(|_| panic!("ICE: arity {} does not fit in u16", arity));

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
            panic!(
                "ICE: over-application of known function chunk {} (arity {}, got {})",
                chunk_id, arity, arg_count
            );
        }
    }

    #[inline(always)]
    fn take_callee_under_args(&mut self, arg_count: usize) -> Value {
        let callee_pos = self.stack.len() - arg_count - 1;
        let callee = self.stack[callee_pos];

        for i in callee_pos..self.stack.len() - 1 {
            self.stack[i] = self.stack[i + 1];
        }
        self.stack.pop();

        callee
    }

    fn invoke_value(
        &mut self,
        program: &BytecodeProgram,
        mode: InvokeMode,
        callee: Value,
        arg_count: usize,
    ) -> Option<Value> {
        match callee {
            Value::CtorFn { tag, arity } => {
                self.maybe_gc();
                Some(self.apply_constructor(tag, arity, PayloadStore::empty(), arg_count))
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
                    panic!(
                        "ICE: over-application of function reference (arity {}, got {})",
                        arity, arg_count
                    );
                }
            }

            Value::Obj(id) => self.invoke_heap_object(program, mode, id, arg_count),

            _ => panic!(
                "ICE: cannot {}call {:?}",
                if matches!(mode, InvokeMode::TailCall) {
                    "tail-"
                } else {
                    ""
                },
                callee
            ),
        }
    }

    fn invoke_heap_object(
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
                            captures,
                            applied_args: PayloadStore::empty(),
                        },
                        arg_count,
                    );
                    None
                } else if arg_count < arity {
                    self.maybe_gc();
                    let applied_args = self.pop_payload(arg_count);
                    let captures = self.heap.clone_payload(captures);

                    Some(self.alloc_heap(HeapObject::PartialApp {
                        chunk,
                        captures,
                        applied_args,
                    }))
                } else {
                    panic!(
                        "ICE: over-application of closure (arity {}, got {})",
                        arity, arg_count
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
                            captures,
                            applied_args,
                        },
                        arg_count,
                    );
                    None
                } else if total < arity {
                    self.maybe_gc();
                    let captures = self.heap.clone_payload(captures);
                    let applied_args = self.heap.alloc_concat_payload_and_stack_tail(
                        applied_args,
                        &mut self.stack,
                        arg_count,
                    );

                    Some(self.alloc_heap(HeapObject::PartialApp {
                        chunk,
                        captures,
                        applied_args,
                    }))
                } else {
                    panic!(
                        "ICE: over-application of partial (arity {}, got {})",
                        arity, total
                    );
                }
            }

            HeapObject::PartialCtor {
                tag,
                arity,
                applied_args,
            } => Some(self.apply_constructor(*tag, *arity, *applied_args, arg_count)),

            HeapObject::Data0 { tag }
            | HeapObject::Data1 { tag, .. }
            | HeapObject::Data2 { tag, .. }
            | HeapObject::DataN { tag, .. } => {
                panic!("ICE: cannot apply saturated constructor tag {}", tag);
            }

            HeapObject::Str { .. } => {
                panic!("ICE: heap string reached callable-object invocation");
            }

            HeapObject::IoAction(_) => {
                panic!("ICE: IO action reached callable-object invocation");
            }
        }
    }

    fn apply_constructor(
        &mut self,
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

            let fields =
                self.heap
                    .alloc_concat_payload_and_stack_tail(existing, &mut self.stack, n_new);
            self.alloc_heap(HeapObject::DataN { tag, fields })
        } else if total < arity as usize {
            let applied_args =
                self.heap
                    .alloc_concat_payload_and_stack_tail(existing, &mut self.stack, n_new);

            self.alloc_heap(HeapObject::PartialCtor {
                tag,
                arity,
                applied_args,
            })
        } else {
            panic!("ICE: over-applied constructor tag {}", tag);
        }
    }

    fn call_builtin(
        &mut self,
        program: &BytecodeProgram,
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

        let args_start = self
            .stack
            .len()
            .checked_sub(arg_count)
            .expect("ICE: builtin call stack underflow");
        let args = self.stack[args_start..].to_vec();
        let mut ctx = VmCtx::new(
            &mut self.heap,
            &program.constructor_arities,
            &self.nullary_ctors,
            self.bools.true_val,
            self.bools.false_val,
        );
        let result = (builtin.run)(&mut ctx, &args)?;
        self.stack.truncate(args_start);

        Ok(result)
    }

    fn call_value_for_result(
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

    fn perform_io(
        &mut self,
        program: &BytecodeProgram,
        action: Value,
        io: &mut IoRuntime<'_>,
    ) -> Result<Value, RuntimeError> {
        match self.expect_io_action(action)? {
            IoAction::Pure(value) => Ok(value),
            IoAction::Bind {
                action,
                continuation,
            } => {
                let value = self.perform_io(program, action, io)?;
                let next = self.call_value_for_result(program, continuation, &[value])?;
                self.perform_io(program, next, io)
            }
            IoAction::Print { text, newline } => {
                let mut text = self.expect_string_value(text, "IO print")?;
                if newline {
                    text.push('\n');
                }
                io.write_str(&text)?;
                Ok(Value::Unit)
            }
            IoAction::ReadLine => {
                let line = io.read_line()?;
                Ok(self.heap.alloc_str(&line))
            }
            IoAction::GetArgs => {
                let mut arg_values = Vec::with_capacity(io.args.len());
                for arg in io.args {
                    arg_values.push(self.heap.alloc_str(arg));
                }

                let mut ctx = VmCtx::new(
                    &mut self.heap,
                    &program.constructor_arities,
                    &self.nullary_ctors,
                    self.bools.true_val,
                    self.bools.false_val,
                );
                ctx.alloc_list_from_values(&arg_values, "getArgs")
            }
            IoAction::ReadFile { path } => {
                let path = self.expect_string_value(path, "readFile")?;
                let resolved = io.resolve_path(&path);
                let contents = fs::read_to_string(&resolved).map_err(|err| {
                    RuntimeError::builtin(format!(
                        "readFile failed for `{}`: {err}",
                        resolved.display()
                    ))
                })?;
                Ok(self.heap.alloc_str(&contents))
            }
            IoAction::WriteFile {
                path,
                contents,
                append,
            } => {
                let builtin = if append { "appendFile" } else { "writeFile" };
                let path = self.expect_string_value(path, builtin)?;
                let contents = self.expect_string_value(contents, builtin)?;
                let resolved = io.resolve_path(&path);
                if append {
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&resolved)
                        .and_then(|mut file| file.write_all(contents.as_bytes()))
                        .map_err(|err| {
                            RuntimeError::builtin(format!(
                                "appendFile failed for `{}`: {err}",
                                resolved.display()
                            ))
                        })?;
                } else {
                    fs::write(&resolved, contents).map_err(|err| {
                        RuntimeError::builtin(format!(
                            "writeFile failed for `{}`: {err}",
                            resolved.display()
                        ))
                    })?;
                }
                Ok(Value::Unit)
            }
        }
    }

    fn expect_io_action(&self, value: Value) -> Result<IoAction, RuntimeError> {
        match value {
            Value::Obj(id) => match self.heap.get(id) {
                HeapObject::IoAction(action) => Ok(*action),
                _ => Err(RuntimeError::builtin(
                    "main returned a non-IO heap value while performing IO".to_string(),
                )),
            },
            _ => Err(RuntimeError::builtin(
                "main returned a non-IO value while performing IO".to_string(),
            )),
        }
    }

    fn expect_string_value(&self, value: Value, context: &str) -> Result<String, RuntimeError> {
        match value {
            Value::Obj(id) => match self.heap.get(id) {
                HeapObject::Str { bytes } => Ok(self.heap.str_value(*bytes).to_string()),
                _ => Err(RuntimeError::builtin(format!("{context} expected String"))),
            },
            _ => Err(RuntimeError::builtin(format!("{context} expected String"))),
        }
    }

    fn dispatch_match(
        &mut self,
        program: &BytecodeProgram,
        arms: &[MatchArm],
        scrutinee: Value,
        else_pc: u32,
    ) {
        for arm in arms {
            match &arm.pattern {
                LowerPattern::Var => {
                    self.store(arm.bind_slots[0], scrutinee);
                    self.pc = arm.target as usize;
                    return;
                }
                LowerPattern::Lit(_) => {
                    self.binding_buffer.clear();

                    if match_pattern(
                        program,
                        &self.heap,
                        &arm.pattern,
                        scrutinee,
                        &mut self.binding_buffer,
                    ) {
                        debug_assert!(arm.bind_slots.is_empty());
                        self.pc = arm.target as usize;
                        return;
                    }
                }

                LowerPattern::Wild => {
                    self.pc = arm.target as usize;
                    return;
                }

                LowerPattern::Con(expected_tag, subs) => {
                    let id = match scrutinee {
                        Value::Obj(id) => id,
                        _ => continue,
                    };

                    let obj = self.heap.get(id);

                    match obj {
                        HeapObject::Data0 { tag } if *tag == *expected_tag && subs.is_empty() => {
                            self.pc = arm.target as usize;
                            return;
                        }

                        HeapObject::Data1 { tag, field }
                            if *tag == *expected_tag && subs.len() == 1 =>
                        {
                            let field = *field;

                            if let Some(bind_count) = try_flat_bind(subs) {
                                debug_assert!(bind_count <= 1);

                                if bind_count == 1 {
                                    self.store(arm.bind_slots[0], field);
                                }

                                self.pc = arm.target as usize;
                                return;
                            }
                        }

                        HeapObject::Data2 {
                            tag,
                            field0,
                            field1,
                        } if *tag == *expected_tag && subs.len() == 2 => {
                            let f0 = *field0;
                            let f1 = *field1;

                            if let Some(_bind_count) = try_flat_bind(subs) {
                                let mut si = 0;

                                if matches!(subs[0], LowerPattern::Var) {
                                    self.store(arm.bind_slots[si], f0);
                                    si += 1;
                                }

                                if matches!(subs[1], LowerPattern::Var) {
                                    self.store(arm.bind_slots[si], f1);
                                }

                                self.pc = arm.target as usize;
                                return;
                            }
                        }

                        HeapObject::DataN { tag, fields } => {
                            if *tag != *expected_tag {
                                continue;
                            }

                            if let Some(bind_count) = try_flat_bind(subs) {
                                let fields = self.heap.payloads.as_slice(*fields);
                                if fields.len() != subs.len() {
                                    continue;
                                }

                                self.binding_buffer.clear();
                                for (i, sub) in subs.iter().enumerate() {
                                    if matches!(sub, LowerPattern::Var) {
                                        self.binding_buffer.push(fields[i]);
                                    }
                                }

                                debug_assert_eq!(self.binding_buffer.len(), bind_count);
                                for i in 0..bind_count {
                                    self.store(arm.bind_slots[i], self.binding_buffer[i]);
                                }

                                self.pc = arm.target as usize;
                                return;
                            }
                        }

                        _ => {}
                    }

                    self.binding_buffer.clear();
                    if match_pattern(
                        program,
                        &self.heap,
                        &arm.pattern,
                        scrutinee,
                        &mut self.binding_buffer,
                    ) {
                        for i in 0..arm.bind_slots.len() {
                            self.store(arm.bind_slots[i], self.binding_buffer[i]);
                        }
                        self.pc = arm.target as usize;
                        return;
                    }
                }
            }
        }

        self.pc = else_pc as usize;
    }
    fn resume_after_return(&mut self, result: Value) -> Option<Value> {
        self.stack.truncate(self.frame_base);

        match self
            .continuations
            .pop()
            .expect("return with empty continuation stack")
        {
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
                self.globals[global_index] = GlobalSlot::Evaluated(result);
                Some(result)
            }

            Continuation::PublishGlobalThenResume {
                global_index,
                chunk,
                pc,
                frame_base,
            } => {
                self.globals[global_index] = GlobalSlot::Evaluated(result);
                self.chunk = chunk;
                self.pc = pc;
                self.frame_base = frame_base;
                self.push(result);
                None
            }
        }
    }

    fn dispatch(&mut self, program: &BytecodeProgram) -> Result<Value, RuntimeError> {
        let mut code = &program.chunks[self.chunk].code;
        loop {
            let instr = unsafe { *code.get_unchecked(self.pc) };
            self.pc += 1;

            match instr {
                Instr::PushUnit => self.push(Value::Unit),
                Instr::PushInt(n) => self.push(Value::Int(n)),
                Instr::PushFloat(n) => self.push(Value::Float(n)),
                Instr::PushChar(ch) => self.push(Value::Char(ch)),
                Instr::PushStr(index) => {
                    self.maybe_gc();
                    let value = program
                        .string_constants
                        .get(index as usize)
                        .unwrap_or_else(|| {
                            panic!("ICE: bytecode references missing string constant {index}")
                        });
                    let value = self.heap.alloc_str(value);
                    self.push(value);
                }
                Instr::LoadLocal(slot) => self.push(self.load(slot)),
                Instr::StoreLocal(slot) => {
                    let val = self.pop();
                    self.store(slot, val);
                }
                Instr::LoadGlobal(index) => {
                    self.push_global_or_start_eval(program, index as usize)?;
                    code = &program.chunks[self.chunk].code;
                }
                Instr::PushCtorFn { tag, arity } => {
                    if arity == 0 {
                        self.push(self.nullary_ctors.get(tag));
                    } else {
                        self.push(Value::CtorFn { tag, arity });
                    }
                }
                Instr::MakeClosure {
                    chunk,
                    capture_count,
                } => {
                    self.maybe_gc();
                    let captures = self.pop_payload(capture_count as usize);
                    let val = self.alloc_heap(HeapObject::Closure {
                        chunk: chunk as usize,
                        captures,
                    });
                    self.push(val);
                }
                Instr::PushFunctionRef { chunk, arity } => {
                    self.push(Value::FunctionRef { chunk, arity });
                }
                Instr::Unary(op) => {
                    let rhs = self.pop_int();
                    self.push(match op {
                        solveig_ast::ast::UnOp::Neg => Value::Int(rhs.wrapping_neg()),
                    });
                }
                Instr::Binary(op) => {
                    let rhs = self.pop_int();
                    let lhs = self.pop_int();
                    let result = match op {
                        solveig_ast::ast::BinOp::Plus => Value::Int(lhs.wrapping_add(rhs)),
                        solveig_ast::ast::BinOp::Minus => Value::Int(lhs.wrapping_sub(rhs)),
                        solveig_ast::ast::BinOp::Mult => Value::Int(lhs.wrapping_mul(rhs)),
                        solveig_ast::ast::BinOp::Div => {
                            Value::Int(builtins::int_div_trap(lhs, rhs, "/")?)
                        }
                        solveig_ast::ast::BinOp::Less => self.bool_value(lhs < rhs),
                        solveig_ast::ast::BinOp::Greater => self.bool_value(lhs > rhs),
                        solveig_ast::ast::BinOp::LessOrEqual => self.bool_value(lhs <= rhs),
                        solveig_ast::ast::BinOp::GreaterOrEqual => self.bool_value(lhs >= rhs),
                        solveig_ast::ast::BinOp::Equal => self.bool_value(lhs == rhs),
                        solveig_ast::ast::BinOp::Mod => {
                            Value::Int(builtins::int_mod_trap(lhs, rhs, "%")?)
                        }
                    };
                    self.push(result);
                }
                Instr::CallBuiltin { id, arg_count } => {
                    let result = self.call_builtin(program, id, arg_count as usize)?;
                    self.push(result);
                }
                Instr::Pack {
                    tag,
                    arity,
                    field_count,
                } => {
                    if arity == 0 {
                        self.push(self.nullary_ctors.get(tag));
                    } else if field_count == 2 {
                        self.maybe_gc();
                        let b = self.pop();
                        let a = self.pop();
                        let val = self.heap.alloc(HeapObject::Data2 {
                            tag,
                            field0: a,
                            field1: b,
                        });
                        self.push(val);
                    } else if field_count == 1 {
                        self.maybe_gc();
                        let a = self.pop();
                        let val = self.heap.alloc(HeapObject::Data1 { tag, field: a });
                        self.push(val);
                    } else {
                        self.maybe_gc();
                        let fields = self.pop_payload(field_count as usize);
                        let val = self.alloc_heap(HeapObject::DataN { tag, fields });
                        self.push(val);
                    }
                }

                Instr::CallN { arg_count } => {
                    let n = arg_count as usize;
                    let callee = self.take_callee_under_args(n);

                    if let Some(val) = self.invoke_value(program, InvokeMode::Call, callee, n) {
                        self.push(val);
                    }

                    code = &program.chunks[self.chunk].code;
                }

                Instr::CallKnown { chunk, arg_count } => {
                    let chunk_id = chunk as usize;
                    let n = arg_count as usize;

                    if let Some(val) =
                        self.invoke_known_function(program, InvokeMode::Call, chunk_id, n)
                    {
                        self.push(val);
                    }

                    code = &program.chunks[self.chunk].code;
                }
                Instr::TailCallN { arg_count } => {
                    let n = arg_count as usize;
                    let callee = self.take_callee_under_args(n);

                    if let Some(val) = self.invoke_value(program, InvokeMode::TailCall, callee, n)
                        && let Some(done) = self.resume_after_return(val)
                    {
                        return Ok(done);
                    }

                    code = &program.chunks[self.chunk].code;
                }
                Instr::TailCallKnown { chunk, arg_count } => {
                    let chunk_id = chunk as usize;
                    let n = arg_count as usize;

                    if let Some(val) =
                        self.invoke_known_function(program, InvokeMode::TailCall, chunk_id, n)
                        && let Some(done) = self.resume_after_return(val)
                    {
                        return Ok(done);
                    }

                    code = &program.chunks[self.chunk].code;
                }
                Instr::Return => {
                    let result = self.pop();

                    if let Some(done) = self.resume_after_return(result) {
                        return Ok(done);
                    }

                    code = &program.chunks[self.chunk].code;
                }
                Instr::Jump(target) => self.pc = target as usize,
                Instr::JumpIfFalse(target) => {
                    let cond = self.pop();
                    if cond == self.bools.false_val {
                        self.pc = target as usize;
                    }
                }
                Instr::Match(table_idx) => {
                    let scrutinee = self.pop();
                    let table = &program.chunks[self.chunk].match_tables[table_idx as usize];
                    self.dispatch_match(program, &table.arms, scrutinee, table.else_target);
                }
                Instr::Trap(code) => return Err(RuntimeError::trap(code)),
            }
        }
    }
}

#[inline]
fn try_flat_bind(subs: &[LowerPattern]) -> Option<usize> {
    let mut count = 0;

    for sub in subs {
        match sub {
            LowerPattern::Var => count += 1,
            LowerPattern::Wild => {}
            LowerPattern::Lit(_) | LowerPattern::Con(..) => return None,
        }
    }

    Some(count)
}

fn match_pattern(
    program: &BytecodeProgram,
    heap: &Heap,
    pattern: &LowerPattern,
    value: Value,
    bindings: &mut Vec<Value>,
) -> bool {
    let checkpoint = bindings.len();

    if match_pattern_inner(program, heap, pattern, value, bindings) {
        true
    } else {
        bindings.truncate(checkpoint);
        false
    }
}

fn match_pattern_inner(
    program: &BytecodeProgram,
    heap: &Heap,
    pattern: &LowerPattern,
    value: Value,
    bindings: &mut Vec<Value>,
) -> bool {
    match pattern {
        LowerPattern::Var => {
            bindings.push(value);
            true
        }
        LowerPattern::Lit(lit_val) => match (lit_val, value) {
            (Literal::Int(expected), Value::Int(actual)) => *expected == actual,
            (Literal::Float(expected), Value::Float(actual)) => {
                expected.to_bits() == actual.to_bits()
            }
            (Literal::Char(expected), Value::Char(actual)) => *expected == actual,
            (Literal::Str(expected), Value::Obj(id)) => match heap.get(id) {
                HeapObject::Str { bytes } => program
                    .string_literal_value(*expected)
                    .is_some_and(|expected| heap.str_value(*bytes) == expected),
                _ => false,
            },
            _ => false,
        },
        LowerPattern::Wild => true,
        LowerPattern::Con(expected_tag, subs) => {
            let id = match value {
                Value::Obj(id) => id,
                _ => return false,
            };

            match heap.get(id) {
                HeapObject::Data0 { tag } => *tag == *expected_tag && subs.is_empty(),
                HeapObject::Data1 { tag, field } => {
                    if *tag != *expected_tag || subs.len() != 1 {
                        return false;
                    }
                    match_pattern_inner(program, heap, &subs[0], *field, bindings)
                }
                HeapObject::Data2 {
                    tag,
                    field0,
                    field1,
                } => {
                    if *tag != *expected_tag || subs.len() != 2 {
                        return false;
                    }
                    let cp = bindings.len();
                    if !match_pattern_inner(program, heap, &subs[0], *field0, bindings) {
                        bindings.truncate(cp);
                        return false;
                    }
                    if !match_pattern_inner(program, heap, &subs[1], *field1, bindings) {
                        bindings.truncate(cp);
                        return false;
                    }
                    true
                }
                HeapObject::DataN { tag, fields } => {
                    let fields = heap.payloads.as_slice(*fields);
                    if *tag != *expected_tag || fields.len() != subs.len() {
                        return false;
                    }
                    let cp = bindings.len();
                    for (sub, &field) in subs.iter().zip(fields.iter()) {
                        if !match_pattern_inner(program, heap, sub, field, bindings) {
                            bindings.truncate(cp);
                            return false;
                        }
                    }
                    true
                }
                _ => false,
            }
        }
    }
}

fn chunk_function_shape(program: &BytecodeProgram, chunk_id: usize) -> (usize, usize) {
    match program.chunk(chunk_id).kind {
        ChunkKind::Function {
            capture_count,
            arity,
        } => (capture_count as usize, arity as usize),
        ChunkKind::GlobalThunk => panic!("ICE: thunk has no arity"),
    }
}

fn chunk_arity(program: &BytecodeProgram, chunk_id: usize) -> usize {
    let (_, arity) = chunk_function_shape(program, chunk_id);
    arity
}

fn find_ctor_tag(arities: &[CtorRuntimeInfo], ctor: CtorId) -> u16 {
    arities
        .iter()
        .find(|info| info.arity == 0 && info.ctor == ctor)
        .map(|info| info.tag)
        .unwrap_or_else(|| panic!("ICE: runtime constructor metadata missing CtorId {ctor:?}"))
}

pub struct EvalResult {
    root: Value,
    heap: Heap,
    constructor_arities: Vec<CtorRuntimeInfo>,
}

pub struct IoEvalResult {
    pub result: EvalResult,
    pub stdout: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    pub code: u16,
    pub message: String,
}

impl RuntimeError {
    fn trap(code: u16) -> Self {
        Self {
            code,
            message: crate::bytecode::trap_message(code),
        }
    }

    pub(crate) fn builtin(message: String) -> Self {
        Self { code: 2, message }
    }

    fn cyclic_global(name: &str) -> Self {
        Self {
            code: 3,
            message: format!("cyclic global `{name}`"),
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RuntimeError {}

pub struct Machine;

impl Machine {
    pub fn exec(program: &CCProgram, entry_name: &str) -> Result<EvalResult, RuntimeError> {
        let bytecode = lower_to_bytecode(program);
        Self::exec_compiled(&bytecode, entry_name)
    }

    pub fn exec_compiled(
        program: &BytecodeProgram,
        entry_name: &str,
    ) -> Result<EvalResult, RuntimeError> {
        let Some(entry) = program.global_index_by_name(entry_name) else {
            return Err(RuntimeError {
                code: 0,
                message: format!("entry '{entry_name}' not found"),
            });
        };

        let mut vm = Vm::new(program);
        let root = vm.eval_global(program, entry)?;

        Ok(EvalResult {
            root,
            heap: vm.heap,
            constructor_arities: program.constructor_arities.clone(),
        })
    }

    pub fn exec_io(
        program: &CCProgram,
        entry_name: &str,
        stdin: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<IoEvalResult, RuntimeError> {
        let bytecode = lower_to_bytecode(program);
        Self::exec_compiled_io(&bytecode, entry_name, stdin, args, cwd)
    }

    pub fn exec_compiled_io(
        program: &BytecodeProgram,
        entry_name: &str,
        stdin: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<IoEvalResult, RuntimeError> {
        let mut input = Cursor::new(stdin.as_bytes());
        let mut stdout = Vec::new();
        let result = Self::exec_compiled_io_with_streams(
            program,
            entry_name,
            &mut input,
            &mut stdout,
            args,
            cwd,
        )?;
        let stdout = String::from_utf8(stdout).map_err(|err| {
            RuntimeError::builtin(format!("captured IO output was not valid UTF-8: {err}"))
        })?;

        Ok(IoEvalResult { result, stdout })
    }

    pub fn exec_io_with_streams(
        program: &CCProgram,
        entry_name: &str,
        stdin: &mut dyn BufRead,
        stdout: &mut dyn Write,
        args: &[String],
        cwd: &Path,
    ) -> Result<EvalResult, RuntimeError> {
        let bytecode = lower_to_bytecode(program);
        Self::exec_compiled_io_with_streams(&bytecode, entry_name, stdin, stdout, args, cwd)
    }

    pub fn exec_compiled_io_with_streams(
        program: &BytecodeProgram,
        entry_name: &str,
        stdin: &mut dyn BufRead,
        stdout: &mut dyn Write,
        args: &[String],
        cwd: &Path,
    ) -> Result<EvalResult, RuntimeError> {
        let Some(entry) = program.global_index_by_name(entry_name) else {
            return Err(RuntimeError {
                code: 0,
                message: format!("entry '{entry_name}' not found"),
            });
        };

        let mut vm = Vm::new(program);
        let mut io = IoRuntime::new(stdin, stdout, args, cwd);
        let action = vm.eval_global(program, entry)?;
        let root = vm.perform_io(program, action, &mut io)?;

        Ok(EvalResult {
            root,
            heap: vm.heap,
            constructor_arities: program.constructor_arities.clone(),
        })
    }

    pub fn dump_bytecode<P: AsRef<std::path::Path>>(
        program: &CCProgram,
        path: P,
    ) -> std::io::Result<()> {
        let bytecode = lower_to_bytecode(program);
        bytecode.dump_to_file(path)
    }
}

impl EvalResult {
    pub fn display(&self) -> DisplayValue<'_> {
        DisplayValue {
            root: self.root,
            heap: &self.heap,
            arities: &self.constructor_arities,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        let Value::Obj(id) = self.root else {
            return None;
        };

        match self.heap.get(id) {
            HeapObject::Str { bytes } => Some(self.heap.str_value(*bytes)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solveig_ast::ast::{BinOp, CtorId, CtorRuntimeRole};
    use solveig_lower::closure_ir::{CCExpr, CCFunction, CCGlobalDef};
    use solveig_lower::{LowerCaseArm, LowerPattern};
    use std::rc::Rc;

    const TRUE_TAG: u16 = 0;
    const FALSE_TAG: u16 = 1;
    const NIL_TAG: u16 = 2;
    const CONS_TAG: u16 = 3;
    const PRELUDE_NIL_TAG: u16 = 4;
    const PRELUDE_CONS_TAG: u16 = 5;
    const PRELUDE_PAIR_TAG: u16 = 6;
    const USER_PAIR_TAG: u16 = 7;

    #[test]
    fn tail_recursive_countdown_uses_bounded_vm_stack() {
        let countdown = function(
            0,
            "countdown",
            2,
            if_then_else(
                bin(BinOp::Equal, local(1), int(0)),
                local(0),
                call_known(
                    0,
                    vec![
                        bin(BinOp::Minus, local(1), int(1)),
                        bin(BinOp::Plus, local(0), int(1)),
                    ],
                ),
            ),
        );

        let program = program(
            vec![global("main", call_known(0, vec![int(5_000_000), int(0)]))],
            vec![countdown],
            &[],
        );

        let bytecode = lower_to_bytecode(&program);
        let countdown_chunk = bytecode
            .chunks
            .iter()
            .find(|chunk| chunk.name == "countdown")
            .expect("countdown chunk should be lowered");

        assert!(
            countdown_chunk
                .code
                .iter()
                .any(|instr| matches!(instr, Instr::TailCallKnown { .. })),
            "countdown must lower its recursive call as TailCallKnown"
        );
        assert!(
            !countdown_chunk
                .code
                .iter()
                .any(|instr| matches!(instr, Instr::CallKnown { .. })),
            "countdown should not keep a caller continuation for the recursive call"
        );

        assert_eq!(run(&program), "5000000");
    }

    #[test]
    fn partial_application_applies_remaining_arguments() {
        let add3 = function(
            0,
            "add3",
            3,
            bin(BinOp::Plus, bin(BinOp::Plus, local(2), local(1)), local(0)),
        );

        let program = program(
            vec![global(
                "main",
                call(call_known(0, vec![int(10), int(20)]), vec![int(12)]),
            )],
            vec![add3],
            &[],
        );

        assert_eq!(run(&program), "42");
    }

    #[test]
    fn gc_preserves_list_across_build_and_fold() {
        let build = function(
            0,
            "build",
            2,
            if_then_else(
                bin(BinOp::Equal, local(1), int(0)),
                local(0),
                call_known(
                    0,
                    vec![
                        bin(BinOp::Minus, local(1), int(1)),
                        pack(CONS_TAG, 2, vec![local(1), local(0)]),
                    ],
                ),
            ),
        );

        let fold_sum = function(
            1,
            "fold_sum",
            2,
            case(
                local(1),
                vec![
                    arm(LowerPattern::Con(NIL_TAG, vec![]), local(0)),
                    arm(
                        LowerPattern::Con(CONS_TAG, vec![LowerPattern::Var, LowerPattern::Var]),
                        call_known(1, vec![local(0), bin(BinOp::Plus, local(2), local(1))]),
                    ),
                ],
            ),
        );

        let program = program(
            vec![global(
                "main",
                let_in(
                    call_known(0, vec![int(40_000), pack(NIL_TAG, 0, Vec::new())]),
                    call_known(1, vec![local(0), int(0)]),
                ),
            )],
            vec![build, fold_sum],
            &[("Nil", NIL_TAG, 0), ("Cons", CONS_TAG, 2)],
        );

        assert_eq!(run(&program), "800020000");
    }

    #[test]
    fn global_thunks_resume_dependency_order() {
        let program = program(
            vec![
                global("main", bin(BinOp::Plus, global_ref(1), global_ref(2))),
                global("branch", bin(BinOp::Plus, global_ref(3), global_ref(2))),
                global("leaf", int(10)),
                global("root", bin(BinOp::Plus, global_ref(2), int(1))),
            ],
            Vec::new(),
            &[],
        );

        assert_eq!(run(&program), "31");
    }

    #[test]
    fn trap_returns_runtime_error() {
        let program = program(
            vec![global("main", case(int(0), Vec::new()))],
            Vec::new(),
            &[],
        );

        let err = match Machine::exec(&program, "main") {
            Ok(_) => panic!("ICE: trap test unexpectedly evaluated successfully"),
            Err(err) => err,
        };

        assert_eq!(err.code, 1);
        assert_eq!(err.message, "non-exhaustive case");
    }

    #[test]
    fn bool_cache_uses_ctor_ids_not_names() {
        let program = program_with_bool_names(
            vec![global(
                "main",
                if_then_else(bin(BinOp::Less, int(1), int(2)), int(42), int(0)),
            )],
            Vec::new(),
            &[],
            "DefinitelyTrue",
            "DefinitelyFalse",
        );

        assert_eq!(run(&program), "42");
    }

    #[test]
    fn display_fallback_prints_structural_constructors_for_prelude_roles() {
        let list = pack(
            PRELUDE_CONS_TAG,
            2,
            vec![
                int(1),
                pack(
                    PRELUDE_CONS_TAG,
                    2,
                    vec![int(2), pack(PRELUDE_NIL_TAG, 0, Vec::new())],
                ),
            ],
        );
        let pair = pack(PRELUDE_PAIR_TAG, 2, vec![int(1), int(2)]);

        let program = program_with_ctors(
            vec![global("listMain", list), global("pairMain", pair)],
            Vec::new(),
            &[
                ctor_spec(
                    CtorId(100),
                    CtorRuntimeRole::PreludeListNil,
                    "renamed.list.Empty",
                    "Empty",
                    PRELUDE_NIL_TAG,
                    0,
                ),
                ctor_spec(
                    CtorId(101),
                    CtorRuntimeRole::PreludeListCons,
                    "renamed.list.Link",
                    "Link",
                    PRELUDE_CONS_TAG,
                    2,
                ),
                ctor_spec(
                    CtorId(102),
                    CtorRuntimeRole::PreludePair,
                    "renamed.pair.Tuple",
                    "Tuple",
                    PRELUDE_PAIR_TAG,
                    2,
                ),
            ],
        );

        assert_eq!(run_entry(&program, "listMain"), "Link 1 (Link 2 Empty)");
        assert_eq!(run_entry(&program, "pairMain"), "Tuple 1 2");
    }

    #[test]
    fn display_does_not_tuple_user_pair_by_name() {
        let program = program_with_ctors(
            vec![global("main", pack(USER_PAIR_TAG, 2, vec![int(1), int(2)]))],
            Vec::new(),
            &[ctor_spec(
                CtorId(200),
                CtorRuntimeRole::Ordinary,
                "main.MkPair",
                "MkPair",
                USER_PAIR_TAG,
                2,
            )],
        );

        assert_eq!(run(&program), "MkPair 1 2");
    }

    fn run(program: &CCProgram) -> String {
        run_entry(program, "main")
    }

    fn run_entry(program: &CCProgram, entry_name: &str) -> String {
        Machine::exec(program, entry_name)
            .expect("VM test program should evaluate successfully")
            .display()
            .to_string()
    }

    fn program(
        globals: Vec<CCGlobalDef>,
        functions: Vec<CCFunction>,
        extra_constructors: &[(&str, u16, u16)],
    ) -> CCProgram {
        program_with_bool_names(globals, functions, extra_constructors, "True", "False")
    }

    fn program_with_bool_names(
        globals: Vec<CCGlobalDef>,
        functions: Vec<CCFunction>,
        extra_constructors: &[(&str, u16, u16)],
        true_name: &'static str,
        false_name: &'static str,
    ) -> CCProgram {
        let extra_constructors = extra_constructors
            .iter()
            .enumerate()
            .map(|(index, (name, tag, arity))| {
                ctor_spec(
                    CtorId(1_000 + index as u32),
                    CtorRuntimeRole::Ordinary,
                    name,
                    name,
                    *tag,
                    *arity,
                )
            })
            .collect::<Vec<_>>();

        program_with_bool_ctor_specs(
            globals,
            functions,
            &extra_constructors,
            true_name,
            false_name,
        )
    }

    fn program_with_ctors(
        globals: Vec<CCGlobalDef>,
        functions: Vec<CCFunction>,
        extra_constructors: &[TestCtorSpec],
    ) -> CCProgram {
        program_with_bool_ctor_specs(globals, functions, extra_constructors, "True", "False")
    }

    fn program_with_bool_ctor_specs(
        globals: Vec<CCGlobalDef>,
        functions: Vec<CCFunction>,
        extra_constructors: &[TestCtorSpec],
        true_name: &'static str,
        false_name: &'static str,
    ) -> CCProgram {
        let true_spec = ctor_spec(
            PRIM_TYS.true_ctor,
            CtorRuntimeRole::Ordinary,
            true_name,
            true_name,
            TRUE_TAG,
            0,
        );
        let false_spec = ctor_spec(
            PRIM_TYS.false_ctor,
            CtorRuntimeRole::Ordinary,
            false_name,
            false_name,
            FALSE_TAG,
            0,
        );
        let mut constructor_arities = vec![
            ctor_runtime_info(&true_spec),
            ctor_runtime_info(&false_spec),
        ];

        constructor_arities.extend(extra_constructors.iter().map(ctor_runtime_info));

        CCProgram {
            globals,
            functions,
            constructor_arities,
            string_constants: Vec::new(),
            string_literals: Vec::new(),
        }
    }

    struct TestCtorSpec {
        ctor: CtorId,
        role: CtorRuntimeRole,
        name: String,
        short_name: String,
        tag: u16,
        arity: u16,
    }

    fn ctor_spec(
        ctor: CtorId,
        role: CtorRuntimeRole,
        name: &str,
        short_name: &str,
        tag: u16,
        arity: u16,
    ) -> TestCtorSpec {
        TestCtorSpec {
            ctor,
            role,
            name: name.to_string(),
            short_name: short_name.to_string(),
            tag,
            arity,
        }
    }

    fn ctor_runtime_info(spec: &TestCtorSpec) -> CtorRuntimeInfo {
        CtorRuntimeInfo {
            ctor: spec.ctor,
            role: spec.role,
            name: spec.name.clone(),
            short_name: spec.short_name.clone(),
            tag: spec.tag,
            arity: spec.arity,
        }
    }

    fn function(id: usize, name: &str, arity: usize, body: Rc<CCExpr>) -> CCFunction {
        CCFunction {
            id,
            name: name.to_string(),
            arity,
            capture_count: 0,
            body,
        }
    }

    fn global(name: &str, body: Rc<CCExpr>) -> CCGlobalDef {
        CCGlobalDef {
            name: name.to_string(),
            body,
        }
    }

    fn expr(expr: CCExpr) -> Rc<CCExpr> {
        Rc::new(expr)
    }

    fn int(n: i64) -> Rc<CCExpr> {
        expr(CCExpr::Int(n))
    }

    fn local(index: usize) -> Rc<CCExpr> {
        expr(CCExpr::Local(index))
    }

    fn global_ref(index: usize) -> Rc<CCExpr> {
        expr(CCExpr::Global(index))
    }

    fn bin(op: BinOp, lhs: Rc<CCExpr>, rhs: Rc<CCExpr>) -> Rc<CCExpr> {
        expr(CCExpr::BinaryOp(op, lhs, rhs))
    }

    fn call(callee: Rc<CCExpr>, args: Vec<Rc<CCExpr>>) -> Rc<CCExpr> {
        expr(CCExpr::Call { callee, args })
    }

    fn call_known(func: usize, args: Vec<Rc<CCExpr>>) -> Rc<CCExpr> {
        expr(CCExpr::CallKnown { func, args })
    }

    fn let_in(binder: Rc<CCExpr>, body: Rc<CCExpr>) -> Rc<CCExpr> {
        expr(CCExpr::Let(binder, body))
    }

    fn pack(tag: u16, arity: u16, fields: Vec<Rc<CCExpr>>) -> Rc<CCExpr> {
        expr(CCExpr::Pack { tag, arity, fields })
    }

    fn case(scrutinee: Rc<CCExpr>, arms: Vec<LowerCaseArm<CCExpr>>) -> Rc<CCExpr> {
        expr(CCExpr::Case { scrutinee, arms })
    }

    fn arm(pattern: LowerPattern, body: Rc<CCExpr>) -> LowerCaseArm<CCExpr> {
        LowerCaseArm { pattern, body }
    }

    fn if_then_else(cond: Rc<CCExpr>, then_br: Rc<CCExpr>, else_br: Rc<CCExpr>) -> Rc<CCExpr> {
        expr(CCExpr::IfThenElse {
            cond,
            then_br,
            else_br,
        })
    }
}
