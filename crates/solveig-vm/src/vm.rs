use solveig_ast::ast::CtorRuntimeInfo;
use solveig_ast::ast::Literal;

use super::bytecode::{
    BytePattern, BytecodeProgram, ChunkKind, Instr, MatchArm, lower_to_bytecode,
};
use super::heap::{BoundPrefix, Heap, HeapObject, PayloadRef, PayloadStore};
use super::values::{BoolCache, Continuation, GlobalSlot, NullaryCtorCache, ObjId, Value};
use crate::display::DisplayValue;
use solveig_lower::closure_ir::CCProgram;

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
    TailCall,
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

        let true_tag = find_bool_tag(arities, "True");
        let false_tag = find_bool_tag(arities, "False");
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
    fn push_bool(&mut self, b: bool) {
        self.push(if b {
            self.bools.true_val
        } else {
            self.bools.false_val
        });
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
    ) -> Option<Value> {
        let thunk_chunk = match &self.globals[index] {
            GlobalSlot::Evaluated(val) => return Some(*val),
            GlobalSlot::Evaluating => panic!("cyclic global {}", index),
            GlobalSlot::Thunk { chunk } => *chunk,
        };

        self.globals[index] = GlobalSlot::Evaluating;
        self.continuations.push(continuation);
        self.enter_thunk(program, thunk_chunk);
        None
    }

    fn eval_global(&mut self, program: &BytecodeProgram, index: usize) -> Value {
        match self.start_global_eval(
            program,
            index,
            Continuation::PublishGlobalThenHalt {
                global_index: index,
            },
        ) {
            Some(val) => val,
            None => self.dispatch(program),
        }
    }

    fn push_global_or_start_eval(&mut self, program: &BytecodeProgram, index: usize) {
        let continuation = Continuation::PublishGlobalThenResume {
            global_index: index,
            chunk: self.chunk,
            pc: self.pc,
            frame_base: self.frame_base,
        };

        if let Some(val) = self.start_global_eval(program, index, continuation) {
            self.push(val);
        }
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
            InvokeMode::Call => {
                self.push_caller_continuation();
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
                "known call to captured function chunk {} (captures {}, arity {})",
                chunk_id, capture_count, arity
            );
        }

        if arg_count == arity {
            self.enter_known_function(program, mode, chunk_id, arg_count);
            None
        } else if arg_count < arity {
            if arg_count == 0 {
                let chunk = u32::try_from(chunk_id)
                    .unwrap_or_else(|_| panic!("chunk id {} does not fit in u32", chunk_id));
                let arity = u16::try_from(arity)
                    .unwrap_or_else(|_| panic!("arity {} does not fit in u16", arity));

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
                "over-application of known function chunk {} (arity {}, got {})",
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
                        "over-application of function reference (arity {}, got {})",
                        arity, arg_count
                    );
                }
            }

            Value::Obj(id) => self.invoke_heap_object(program, mode, id, arg_count),

            _ => panic!(
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
                        "over-application of closure (arity {}, got {})",
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
                        "over-application of partial (arity {}, got {})",
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
                panic!("cannot apply saturated constructor tag {}", tag);
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
            panic!("over-applied constructor tag {}", tag);
        }
    }

    fn dispatch_match(&mut self, arms: &[MatchArm], scrutinee: Value, else_pc: u32) {
        for arm in arms {
            match &arm.pattern {
                BytePattern::Var => {
                    self.store(arm.bind_slots[0], scrutinee);
                    self.pc = arm.target as usize;
                    return;
                }
                BytePattern::Lit(_) => {
                    self.binding_buffer.clear();

                    if match_pattern(
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

                BytePattern::Wild => {
                    self.pc = arm.target as usize;
                    return;
                }

                BytePattern::Con(expected_tag, subs) => {
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

                                if matches!(subs[0], BytePattern::Var) {
                                    self.store(arm.bind_slots[si], f0);
                                    si += 1;
                                }

                                if matches!(subs[1], BytePattern::Var) {
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
                                    if matches!(sub, BytePattern::Var) {
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

    fn dispatch(&mut self, program: &BytecodeProgram) -> Value {
        let mut code = &program.chunks[self.chunk].code;
        loop {
            let instr = unsafe { *code.get_unchecked(self.pc) };
            self.pc += 1;

            match instr {
                Instr::PushUnit => self.push(Value::Unit),
                Instr::PushInt(n) => self.push(Value::Int(n)),
                Instr::LoadLocal(slot) => self.push(self.load(slot)),
                Instr::StoreLocal(slot) => {
                    let val = self.pop();
                    self.store(slot, val);
                }
                Instr::LoadGlobal(index) => {
                    self.push_global_or_start_eval(program, index as usize);
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
                        solveig_ast::ast::UnOp::Neg => Value::Int(-rhs),
                    });
                }
                Instr::Binary(op) => {
                    let rhs = self.pop_int();
                    let lhs = self.pop_int();
                    match op {
                        solveig_ast::ast::BinOp::Plus => self.push(Value::Int(lhs + rhs)),
                        solveig_ast::ast::BinOp::Minus => self.push(Value::Int(lhs - rhs)),
                        solveig_ast::ast::BinOp::Mult => self.push(Value::Int(lhs * rhs)),
                        solveig_ast::ast::BinOp::Div => self.push(Value::Int(lhs / rhs)),
                        solveig_ast::ast::BinOp::Less => self.push_bool(lhs < rhs),
                        solveig_ast::ast::BinOp::Greater => self.push_bool(lhs > rhs),
                        solveig_ast::ast::BinOp::LessOrEqual => self.push_bool(lhs <= rhs),
                        solveig_ast::ast::BinOp::GreaterOrEqual => self.push_bool(lhs >= rhs),
                        solveig_ast::ast::BinOp::Equal => self.push_bool(lhs == rhs),
                        solveig_ast::ast::BinOp::Mod => self.push(Value::Int(lhs % rhs)),
                    }
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
                        return done;
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
                        return done;
                    }

                    code = &program.chunks[self.chunk].code;
                }
                Instr::Return => {
                    let result = self.pop();

                    if let Some(done) = self.resume_after_return(result) {
                        return done;
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
                    self.dispatch_match(&table.arms, scrutinee, table.else_target);
                }
                Instr::Trap(msg) => panic!("{}", msg),
            }
        }
    }
}

#[inline]
fn try_flat_bind(subs: &[BytePattern]) -> Option<usize> {
    let mut count = 0;

    for sub in subs {
        match sub {
            BytePattern::Var => count += 1,
            BytePattern::Wild => {}
            BytePattern::Lit(_) | BytePattern::Con(..) => return None,
        }
    }

    Some(count)
}

fn match_pattern(
    heap: &Heap,
    pattern: &BytePattern,
    value: Value,
    bindings: &mut Vec<Value>,
) -> bool {
    let checkpoint = bindings.len();

    if match_pattern_inner(heap, pattern, value, bindings) {
        true
    } else {
        bindings.truncate(checkpoint);
        false
    }
}

fn match_pattern_inner(
    heap: &Heap,
    pattern: &BytePattern,
    value: Value,
    bindings: &mut Vec<Value>,
) -> bool {
    match pattern {
        BytePattern::Var => {
            bindings.push(value);
            true
        }
        BytePattern::Lit(lit_val) => match (lit_val, value) {
            (Literal::Int(expected), Value::Int(actual)) => *expected == actual,
            _ => false,
        },
        BytePattern::Wild => true,
        BytePattern::Con(expected_tag, subs) => {
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
                    match_pattern_inner(heap, &subs[0], *field, bindings)
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
                    if !match_pattern_inner(heap, &subs[0], *field0, bindings) {
                        bindings.truncate(cp);
                        return false;
                    }
                    if !match_pattern_inner(heap, &subs[1], *field1, bindings) {
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
                        if !match_pattern_inner(heap, sub, field, bindings) {
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
        ChunkKind::GlobalThunk => panic!("thunk has no arity"),
    }
}

fn chunk_arity(program: &BytecodeProgram, chunk_id: usize) -> usize {
    let (_, arity) = chunk_function_shape(program, chunk_id);
    arity
}

fn find_bool_tag(arities: &[CtorRuntimeInfo], name: &str) -> u16 {
    arities
        .iter()
        .find(|info| info.arity == 0 && (info.name == name || info.short_name == name))
        .map(|info| info.tag)
        .unwrap_or_else(|| panic!("'{}' constructor not found", name))
}

pub struct EvalResult {
    root: Value,
    heap: Heap,
    constructor_arities: Vec<CtorRuntimeInfo>,
}

pub struct Machine;

impl Machine {
    pub fn exec(program: &CCProgram, entry_name: &str) -> EvalResult {
        let bytecode = lower_to_bytecode(program);
        Self::exec_compiled(&bytecode, entry_name)
    }

    pub fn exec_compiled(program: &BytecodeProgram, entry_name: &str) -> EvalResult {
        let entry = program
            .global_index_by_name(entry_name)
            .unwrap_or_else(|| panic!("entry '{}' not found", entry_name));

        let mut vm = Vm::new(program);
        let root = vm.eval_global(program, entry);

        EvalResult {
            root,
            heap: vm.heap,
            constructor_arities: program.constructor_arities.clone(),
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use solveig_ast::ast::{BinOp, PRIM_TYS, legacy_ctor_id};
    use solveig_lower::closure_ir::{CCCaseArm, CCExpr, CCFunction, CCGlobalDef, CCPattern};
    use std::rc::Rc;

    const TRUE_TAG: u16 = 0;
    const FALSE_TAG: u16 = 1;
    const NIL_TAG: u16 = 2;
    const CONS_TAG: u16 = 3;

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
                    arm(CCPattern::Con(NIL_TAG, vec![]), local(0)),
                    arm(
                        CCPattern::Con(CONS_TAG, vec![CCPattern::Var, CCPattern::Var]),
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

    fn run(program: &CCProgram) -> String {
        Machine::exec(program, "main").display().to_string()
    }

    fn program(
        globals: Vec<CCGlobalDef>,
        functions: Vec<CCFunction>,
        extra_constructors: &[(&str, u16, u16)],
    ) -> CCProgram {
        let mut constructor_arities = vec![
            CtorRuntimeInfo {
                ctor: PRIM_TYS.true_ctor,
                name: "True".to_string(),
                short_name: "True".to_string(),
                tag: TRUE_TAG,
                arity: 0,
            },
            CtorRuntimeInfo {
                ctor: PRIM_TYS.false_ctor,
                name: "False".to_string(),
                short_name: "False".to_string(),
                tag: FALSE_TAG,
                arity: 0,
            },
        ];

        constructor_arities.extend(extra_constructors.iter().map(|(name, tag, arity)| {
            CtorRuntimeInfo {
                ctor: legacy_ctor_id(name),
                name: (*name).to_string(),
                short_name: (*name).to_string(),
                tag: *tag,
                arity: *arity,
            }
        }));

        CCProgram {
            globals,
            functions,
            constructor_arities,
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

    fn case(scrutinee: Rc<CCExpr>, arms: Vec<CCCaseArm>) -> Rc<CCExpr> {
        expr(CCExpr::Case { scrutinee, arms })
    }

    fn arm(pattern: CCPattern, body: Rc<CCExpr>) -> CCCaseArm {
        CCCaseArm { pattern, body }
    }

    fn if_then_else(cond: Rc<CCExpr>, then_br: Rc<CCExpr>, else_br: Rc<CCExpr>) -> Rc<CCExpr> {
        expr(CCExpr::IfThenElse {
            cond,
            then_br,
            else_br,
        })
    }
}
