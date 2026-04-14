use super::bytecode::{
    BytePattern, BytecodeProgram, ChunkKind, Instr, MatchArm, lower_to_bytecode,
};
use super::heap::{Heap, HeapObject, PayloadRef, PayloadStore};
use super::values::{BoolCache, GlobalSlot, NullaryCache, ObjId, ReturnTo, Value};
use crate::display::DisplayValue;
use lowering::closure_ir::CCProgram;

struct Vm {
    chunk: usize,
    pc: usize,
    frame_base: usize,

    stack: Vec<Value>,
    returns: Vec<ReturnTo>,
    match_buffer: Vec<Value>,

    heap: Heap,
    globals: Vec<GlobalSlot>,
    nullary: NullaryCache,
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

impl Vm {
    fn new(program: &BytecodeProgram) -> Self {
        let max_tag = program
            .constructor_arities
            .iter()
            .map(|(_, tag, _)| *tag as usize)
            .max()
            .unwrap_or(0);

        let mut vm = Vm {
            chunk: 0,
            pc: 0,
            frame_base: 0,
            stack: Vec::with_capacity(4096),
            returns: Vec::with_capacity(512),
            match_buffer: Vec::with_capacity(8),
            heap: Heap::default(),
            globals: program
                .globals
                .iter()
                .map(|g| GlobalSlot::Thunk { chunk: g.chunk })
                .collect(),
            nullary: NullaryCache::new(max_tag),
            bools: BoolCache {
                true_val: Value::Unit,
                false_val: Value::Unit,
            },
        };

        vm.init_nullary(&program.constructor_arities);
        vm
    }

    fn init_nullary(&mut self, arities: &[(String, u16, u16)]) {
        for &(_, tag, arity) in arities {
            if arity == 0 {
                let val = self.alloc_heap(HeapObject::Data {
                    tag,
                    fields: PayloadStore::empty(),
                });
                self.nullary.set(tag, val);
            }
        }

        let true_tag = find_bool_tag(arities, "True");
        let false_tag = find_bool_tag(arities, "False");
        self.bools.true_val = self.nullary.get(true_tag);
        self.bools.false_val = self.nullary.get(false_tag);
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

        for &val in &self.match_buffer {
            if let Value::Obj(id) = val {
                mark_bit(&mut self.heap.mark_bits, id, &mut worklist);
            }
        }

        for i in 0..self.globals.len() {
            if let GlobalSlot::Evaluated(Value::Obj(id)) = self.globals[i] {
                mark_bit(&mut self.heap.mark_bits, id, &mut worklist);
            }
        }

        for slot in &self.nullary.tags {
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
                    captures, supplied, ..
                }) => Children::Two(*captures, *supplied),
                Some(HeapObject::PartialCtor { supplied, .. }) => Children::One(*supplied),
                Some(HeapObject::Data { fields, .. }) => Children::One(*fields),
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
    fn push_return(&mut self) {
        self.returns.push(ReturnTo::Caller {
            chunk: self.chunk,
            pc: self.pc,
            frame_base: self.frame_base,
        });
    }

    fn force_global(&mut self, program: &BytecodeProgram, index: usize) -> Value {
        match &self.globals[index] {
            GlobalSlot::Evaluated(val) => return *val,
            GlobalSlot::BlackHole => panic!("cyclic global {}", index),
            GlobalSlot::Thunk { .. } => {}
        }

        let thunk_chunk = match &self.globals[index] {
            GlobalSlot::Thunk { chunk } => *chunk,
            _ => unreachable!(),
        };

        self.globals[index] = GlobalSlot::BlackHole;
        self.returns.push(ReturnTo::UpdateThenHalt {
            global_index: index,
        });
        self.enter_thunk(program, thunk_chunk);
        self.dispatch(program)
    }

    fn force_global_inline(&mut self, program: &BytecodeProgram, index: usize) {
        match &self.globals[index] {
            GlobalSlot::Evaluated(val) => {
                self.push(*val);
                return;
            }
            GlobalSlot::BlackHole => panic!("cyclic global {}", index),
            GlobalSlot::Thunk { .. } => {}
        }

        let thunk_chunk = match &self.globals[index] {
            GlobalSlot::Thunk { chunk } => *chunk,
            _ => unreachable!(),
        };

        self.globals[index] = GlobalSlot::BlackHole;
        self.returns.push(ReturnTo::UpdateThenResume {
            global_index: index,
            chunk: self.chunk,
            pc: self.pc,
            frame_base: self.frame_base,
        });
        self.enter_thunk(program, thunk_chunk);
    }

    fn enter_thunk(&mut self, program: &BytecodeProgram, chunk_id: usize) {
        let meta = program.chunk(chunk_id);
        self.chunk = chunk_id;
        self.pc = 0;
        self.frame_base = self.stack.len();
        self.stack
            .resize(self.frame_base + meta.slot_count as usize, Value::Unit);
    }

    fn enter_closure(
        &mut self,
        program: &BytecodeProgram,
        chunk_id: usize,
        captures: PayloadRef,
        arg_count: usize,
    ) {
        let meta = program.chunk(chunk_id);
        let slot_count = meta.slot_count as usize;
        let capture_count = PayloadStore::len(&captures);

        let args_start = self.stack.len() - arg_count;
        self.chunk = chunk_id;
        self.pc = 0;
        self.frame_base = args_start;
        self.ensure_slots(slot_count);

        if capture_count > 0 {
            for i in (0..arg_count).rev() {
                self.stack[self.frame_base + capture_count + i] = self.stack[self.frame_base + i];
            }
            let frame_base = self.frame_base;
            let (heap, stack) = (&self.heap, &mut self.stack);
            heap.copy_payload_to_stack(captures, stack, frame_base);
        }
    }

    fn enter_with_prefix_and_stack_tail(
        &mut self,
        program: &BytecodeProgram,
        chunk_id: usize,
        captures: PayloadRef,
        supplied: PayloadRef,
        new_arg_count: usize,
    ) {
        let meta = program.chunk(chunk_id);
        let slot_count = meta.slot_count as usize;

        let capture_len = self.heap.payload_len(&captures);
        let supplied_len = self.heap.payload_len(&supplied);
        let prefix_len = capture_len + supplied_len;

        let args_start = self.stack.len() - new_arg_count;
        self.chunk = chunk_id;
        self.pc = 0;
        self.frame_base = args_start;
        self.ensure_slots(slot_count);

        if prefix_len > 0 {
            for i in (0..new_arg_count).rev() {
                self.stack[self.frame_base + prefix_len + i] = self.stack[self.frame_base + i];
            }

            let frame_base = self.frame_base;
            let (heap, stack) = (&self.heap, &mut self.stack);

            heap.copy_payload_to_stack(captures, stack, frame_base);
            heap.copy_payload_to_stack(supplied, stack, frame_base + capture_len);
        }
    }
    fn tail_enter_closure(
        &mut self,
        program: &BytecodeProgram,
        chunk_id: usize,
        captures: PayloadRef,
        arg_count: usize,
    ) {
        let meta = program.chunk(chunk_id);
        let slot_count = meta.slot_count as usize;
        let capture_count = PayloadStore::len(&captures);

        let args_start = self.stack.len() - arg_count;
        if args_start != self.frame_base {
            for i in 0..arg_count {
                self.stack[self.frame_base + i] = self.stack[args_start + i];
            }
        }

        self.stack.truncate(self.frame_base + arg_count);
        self.chunk = chunk_id;
        self.pc = 0;
        self.ensure_slots(slot_count);

        if capture_count > 0 {
            for i in (0..arg_count).rev() {
                self.stack[self.frame_base + capture_count + i] = self.stack[self.frame_base + i];
            }
            let frame_base = self.frame_base;
            let (heap, stack) = (&self.heap, &mut self.stack);
            heap.copy_payload_to_stack(captures, stack, frame_base);
        }
    }

    fn tail_enter_with_prefix_and_stack_tail(
        &mut self,
        program: &BytecodeProgram,
        chunk_id: usize,
        captures: PayloadRef,
        supplied: PayloadRef,
        new_arg_count: usize,
    ) {
        let meta = program.chunk(chunk_id);
        let slot_count = meta.slot_count as usize;

        let capture_len = self.heap.payload_len(&captures);
        let supplied_len = self.heap.payload_len(&supplied);
        let prefix_len = capture_len + supplied_len;

        let args_start = self.stack.len() - new_arg_count;

        if args_start != self.frame_base {
            for i in 0..new_arg_count {
                self.stack[self.frame_base + i] = self.stack[args_start + i];
            }
        }

        self.stack.truncate(self.frame_base + new_arg_count);
        self.chunk = chunk_id;
        self.pc = 0;
        self.ensure_slots(slot_count);

        if prefix_len > 0 {
            for i in (0..new_arg_count).rev() {
                self.stack[self.frame_base + prefix_len + i] = self.stack[self.frame_base + i];
            }

            let frame_base = self.frame_base;
            let (heap, stack) = (&self.heap, &mut self.stack);
            heap.copy_payload_to_stack(captures, stack, frame_base);
            heap.copy_payload_to_stack(supplied, stack, frame_base + capture_len);
        }
    }

    fn tail_apply_callee(
        &mut self,
        program: &BytecodeProgram,
        callee: Value,
        arg_count: usize,
    ) -> Option<Value> {
        self.maybe_gc();
        match callee {
            Value::CtorFn { tag, arity } => {
                Some(self.build_ctor(tag, arity, PayloadStore::empty(), arg_count))
            }

            Value::Obj(id) => self.tail_apply_heap_callee(program, id, arg_count),

            _ => panic!("cannot tail-call {:?}", callee),
        }
    }

    fn tail_apply_heap_callee(
        &mut self,
        program: &BytecodeProgram,
        id: ObjId,
        arg_count: usize,
    ) -> Option<Value> {
        match self.heap.get(id) {
            HeapObject::Closure { chunk, captures } => {
                let chunk = *chunk;
                let captures = *captures;
                let arity = chunk_arity(program, chunk);
                if arg_count == arity {
                    self.tail_enter_closure(program, chunk, captures, arg_count);
                    None
                } else if arg_count < arity {
                    self.maybe_gc();
                    let supplied = self.pop_payload(arg_count);
                    let captures = self.heap.clone_payload(captures);
                    Some(self.alloc_heap(HeapObject::PartialApp {
                        chunk,
                        captures,
                        supplied,
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
                supplied,
            } => {
                let chunk = *chunk;
                let captures = *captures;
                let supplied = *supplied;
                let arity = chunk_arity(program, chunk);
                let total = PayloadStore::len(&supplied) + arg_count;
                if total == arity {
                    self.tail_enter_with_prefix_and_stack_tail(
                        program, chunk, captures, supplied, arg_count,
                    );
                    None
                } else if total < arity {
                    self.maybe_gc();
                    let captures = self.heap.clone_payload(captures);
                    let supplied = self.heap.alloc_concat_payload_and_stack_tail(
                        supplied,
                        &mut self.stack,
                        arg_count,
                    );
                    Some(self.alloc_heap(HeapObject::PartialApp {
                        chunk,
                        captures,
                        supplied,
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
                supplied,
            } => {
                let tag = *tag;
                let arity = *arity;
                let supplied = *supplied;
                Some(self.build_ctor(tag, arity, supplied, arg_count))
            }
            HeapObject::Data { tag, .. } => {
                panic!("cannot apply saturated constructor tag {}", tag);
            }
        }
    }

    fn apply_callee(
        &mut self,
        program: &BytecodeProgram,
        callee: Value,
        arg_count: usize,
    ) -> Option<Value> {
        self.maybe_gc();
        match callee {
            Value::CtorFn { tag, arity } => {
                Some(self.build_ctor(tag, arity, PayloadStore::empty(), arg_count))
            }

            Value::Obj(id) => self.apply_heap_callee(program, id, arg_count),

            _ => panic!("cannot call {:?}", callee),
        }
    }

    fn apply_heap_callee(
        &mut self,
        program: &BytecodeProgram,
        id: ObjId,
        arg_count: usize,
    ) -> Option<Value> {
        match self.heap.get(id) {
            HeapObject::Closure { chunk, captures } => {
                let chunk = *chunk;
                let captures = *captures;
                let arity = chunk_arity(program, chunk);
                if arg_count == arity {
                    self.push_return();
                    self.enter_closure(program, chunk, captures, arg_count);
                    None
                } else if arg_count < arity {
                    self.maybe_gc();
                    let supplied = self.pop_payload(arg_count);
                    let captures = self.heap.clone_payload(captures);
                    Some(self.alloc_heap(HeapObject::PartialApp {
                        chunk,
                        captures,
                        supplied,
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
                supplied,
            } => {
                let chunk = *chunk;
                let captures = *captures;
                let supplied = *supplied;
                let arity = chunk_arity(program, chunk);
                let total = PayloadStore::len(&supplied) + arg_count;
                if total == arity {
                    self.push_return();
                    self.enter_with_prefix_and_stack_tail(
                        program, chunk, captures, supplied, arg_count,
                    );
                    None
                } else if total < arity {
                    self.maybe_gc();
                    let captures = self.heap.clone_payload(captures);
                    let supplied = self.heap.alloc_concat_payload_and_stack_tail(
                        supplied,
                        &mut self.stack,
                        arg_count,
                    );
                    Some(self.alloc_heap(HeapObject::PartialApp {
                        chunk,
                        captures,
                        supplied,
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
                supplied,
            } => Some(self.build_ctor(*tag, *arity, *supplied, arg_count)),
            HeapObject::Data { tag, .. } => {
                panic!("cannot apply saturated constructor tag {}", tag);
            }
        }
    }

    fn build_ctor(&mut self, tag: u16, arity: u16, existing: PayloadRef, n_new: usize) -> Value {
        let total = self.heap.payload_len(&existing) + n_new;
        let fields =
            self.heap
                .alloc_concat_payload_and_stack_tail(existing, &mut self.stack, n_new);

        if total == arity as usize {
            self.alloc_heap(HeapObject::Data { tag, fields })
        } else if total < arity as usize {
            self.alloc_heap(HeapObject::PartialCtor {
                tag,
                arity,
                supplied: fields,
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
                BytePattern::Wild => {
                    self.pc = arm.target as usize;
                    return;
                }
                BytePattern::Con(expected_tag, subs) => {
                    let id = match scrutinee {
                        Value::Obj(id) => id,
                        _ => continue,
                    };
                    let (tag, fields_ref) = match self.heap.get(id) {
                        HeapObject::Data { tag, fields } => (*tag, *fields),
                        _ => continue,
                    };
                    if tag != *expected_tag {
                        continue;
                    }

                    if let Some(bind_count) = try_flat_bind(subs) {
                        let fields = self.heap.payloads.as_slice(fields_ref);
                        if fields.len() != subs.len() {
                            continue;
                        }

                        self.match_buffer.clear();
                        for (i, sub) in subs.iter().enumerate() {
                            if matches!(sub, BytePattern::Var) {
                                self.match_buffer.push(fields[i]);
                            }
                        }

                        debug_assert_eq!(self.match_buffer.len(), bind_count);
                        for i in 0..bind_count {
                            self.store(arm.bind_slots[i], self.match_buffer[i]);
                        }
                        self.pc = arm.target as usize;
                        return;
                    }

                    self.match_buffer.clear();
                    if match_pattern(&self.heap, &arm.pattern, scrutinee, &mut self.match_buffer) {
                        for i in 0..arm.bind_slots.len() {
                            self.store(arm.bind_slots[i], self.match_buffer[i]);
                        }
                        self.pc = arm.target as usize;
                        return;
                    }
                }
            }
        }
        self.pc = else_pc as usize;
    }

    fn finish_return(&mut self, result: Value) -> Option<Value> {
        self.stack.truncate(self.frame_base);

        match self.returns.pop().expect("return with empty return stack") {
            ReturnTo::Halt => Some(result),
            ReturnTo::Caller {
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
            ReturnTo::UpdateThenHalt { global_index } => {
                self.globals[global_index] = GlobalSlot::Evaluated(result);
                Some(result)
            }
            ReturnTo::UpdateThenResume {
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
                    self.force_global_inline(program, index as usize);
                    code = &program.chunks[self.chunk].code;
                }
                Instr::PushCtorFn { tag, arity } => {
                    if arity == 0 {
                        self.push(self.nullary.get(tag));
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
                Instr::Unary(op) => {
                    let rhs = self.pop_int();
                    self.push(match op {
                        core::ast::UnOp::Neg => Value::Int(-rhs),
                    });
                }
                Instr::Binary(op) => {
                    let rhs = self.pop_int();
                    let lhs = self.pop_int();
                    match op {
                        core::ast::BinOp::Plus => self.push(Value::Int(lhs + rhs)),
                        core::ast::BinOp::Minus => self.push(Value::Int(lhs - rhs)),
                        core::ast::BinOp::Mult => self.push(Value::Int(lhs * rhs)),
                        core::ast::BinOp::Div => self.push(Value::Int(lhs / rhs)),
                        core::ast::BinOp::Less => self.push_bool(lhs < rhs),
                        core::ast::BinOp::Greater => self.push_bool(lhs > rhs),
                        core::ast::BinOp::LessOrEqual => self.push_bool(lhs <= rhs),
                        core::ast::BinOp::GreaterOrEqual => self.push_bool(lhs >= rhs),
                        core::ast::BinOp::Equal => self.push_bool(lhs == rhs),
                        core::ast::BinOp::Mod => self.push(Value::Int(lhs % rhs)),
                    }
                }
                Instr::Pack {
                    tag,
                    arity,
                    field_count,
                } => {
                    if arity == 0 {
                        self.push(self.nullary.get(tag));
                    } else if field_count == 2 {
                        self.maybe_gc();
                        let b = self.pop();
                        let a = self.pop();
                        let span = if let Some(start) = self.heap.payloads.bucket2.pop() {
                            start..(start + 2)
                        } else {
                            let start = self.heap.payloads.values.len();
                            self.heap.payloads.values.reserve(2);
                            unsafe {
                                self.heap.payloads.values.set_len(start + 2);
                            }
                            start..(start + 2)
                        };
                        self.heap.payloads.values[span.start] = a;
                        self.heap.payloads.values[span.start + 1] = b;
                        let val = self.heap.alloc(HeapObject::Data {
                            tag,
                            fields: PayloadRef {
                                start: span.start,
                                end: span.end,
                            },
                        });
                        self.push(val);
                    } else {
                        self.maybe_gc();
                        let fields = self.pop_payload(field_count as usize);
                        let val = self.alloc_heap(HeapObject::Data { tag, fields });
                        self.push(val);
                    }
                }

                Instr::CallN { arg_count } => {
                    let n = arg_count as usize;
                    let callee_pos = self.stack.len() - n - 1;
                    let callee = self.stack[callee_pos];

                    for i in callee_pos..self.stack.len() - 1 {
                        self.stack[i] = self.stack[i + 1];
                    }
                    self.stack.pop();

                    if let Some(val) = self.apply_callee(program, callee, n) {
                        self.push(val);
                    }
                    code = &program.chunks[self.chunk].code;
                }

                Instr::CallKnown { chunk, arg_count } => {
                    let chunk_id = chunk as usize;
                    let n = arg_count as usize;

                    self.push_return();

                    let meta = program.chunk(chunk_id);
                    let slot_count = meta.slot_count as usize;
                    let capture_count = match meta.kind {
                        ChunkKind::Function { capture_count, .. } => capture_count as usize,
                        _ => 0,
                    };

                    let args_start = self.stack.len() - n;
                    self.chunk = chunk_id;
                    self.pc = 0;
                    self.frame_base = args_start;
                    self.ensure_slots(slot_count);

                    if capture_count > 0 {
                        for i in (0..n).rev() {
                            self.stack[self.frame_base + capture_count + i] =
                                self.stack[self.frame_base + i];
                        }
                    }

                    code = &program.chunks[self.chunk].code;
                }
                Instr::TailCallN { arg_count } => {
                    let n = arg_count as usize;
                    let callee_pos = self.stack.len() - n - 1;
                    let callee = self.stack[callee_pos];

                    for i in callee_pos..self.stack.len() - 1 {
                        self.stack[i] = self.stack[i + 1];
                    }
                    self.stack.pop();

                    if let Some(val) = self.tail_apply_callee(program, callee, n)
                        && let Some(done) = self.finish_return(val)
                    {
                        return done;
                    }
                    code = &program.chunks[self.chunk].code;
                }

                Instr::TailCallKnown { chunk, arg_count } => {
                    let chunk_id = chunk as usize;
                    let n = arg_count as usize;
                    let args_start = self.stack.len() - n;

                    if args_start != self.frame_base {
                        for i in 0..n {
                            self.stack[self.frame_base + i] = self.stack[args_start + i];
                        }
                    }

                    self.stack.truncate(self.frame_base + n);
                    self.pc = 0;

                    if chunk_id != self.chunk {
                        self.chunk = chunk_id;
                        let slot_count = program.chunks[chunk_id].slot_count as usize;
                        self.ensure_slots(slot_count);
                        code = &program.chunks[chunk_id].code;
                    } else {
                        let slot_count = program.chunks[chunk_id].slot_count as usize;
                        self.ensure_slots(slot_count);
                    }
                }
                Instr::Return => {
                    let result = self.pop();
                    self.stack.truncate(self.frame_base);

                    match self.returns.pop().expect("empty return stack") {
                        ReturnTo::Halt => return result,
                        ReturnTo::Caller {
                            chunk,
                            pc,
                            frame_base,
                        } => {
                            self.chunk = chunk;
                            self.pc = pc;
                            self.frame_base = frame_base;
                            self.push(result);
                            code = &program.chunks[chunk].code;
                        }
                        ReturnTo::UpdateThenHalt { global_index } => {
                            self.globals[global_index] = GlobalSlot::Evaluated(result);
                            return result;
                        }
                        ReturnTo::UpdateThenResume {
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
                            code = &program.chunks[chunk].code;
                        }
                    }
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
            BytePattern::Con(..) => return None,
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

        BytePattern::Wild => true,
        BytePattern::Con(expected_tag, subs) => {
            let id = match value {
                Value::Obj(id) => id,
                _ => return false,
            };

            let (tag, fields) = match heap.get(id) {
                HeapObject::Data { tag, fields } => (*tag, heap.payloads.as_slice(*fields)),
                _ => return false,
            };

            if tag != *expected_tag || fields.len() != subs.len() {
                return false;
            }

            let checkpoint = bindings.len();

            for (sub, &field) in subs.iter().zip(fields.iter()) {
                if !match_pattern_inner(heap, sub, field, bindings) {
                    bindings.truncate(checkpoint);
                    return false;
                }
            }

            true
        }
    }
}

fn chunk_arity(program: &BytecodeProgram, chunk_id: usize) -> usize {
    match program.chunk(chunk_id).kind {
        ChunkKind::Function { arity, .. } => arity as usize,
        ChunkKind::GlobalThunk => panic!("thunk has no arity"),
    }
}

fn find_bool_tag(arities: &[(String, u16, u16)], name: &str) -> u16 {
    arities
        .iter()
        .find(|(n, _, a)| *a == 0 && (n.ends_with(&format!(".{}", name)) || n == name))
        .map(|(_, tag, _)| *tag)
        .unwrap_or_else(|| panic!("'{}' constructor not found", name))
}

pub struct EvalResult {
    root: Value,
    heap: Heap,
    constructor_arities: Vec<(String, u16, u16)>,
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
        let root = vm.force_global(program, entry);

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
