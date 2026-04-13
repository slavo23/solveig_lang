use std::fmt;

use super::bytecode::{
    lower_to_bytecode, BytePattern, BytecodeProgram, ChunkKind, Instr, MatchArm,
};
use super::closure_ir::CCProgram;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ObjId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Value {
    Unit,
    Int(i64),
    CtorFn { tag: u16, arity: u16 },
    Obj(ObjId),
}

use std::ops::Range;

type PayloadRef = Range<usize>;

#[derive(Debug, Default)]
struct PayloadStore {
    values: Vec<Value>,
    free: Vec<PayloadRef>,
    bucket1: Vec<usize>,
    bucket2: Vec<usize>,
    bucket3: Vec<usize>,
}

impl PayloadStore {
    #[inline]
    fn empty() -> PayloadRef {
        0..0
    }

    #[inline]
    fn len(span: &PayloadRef) -> usize {
        span.end - span.start
    }

    #[inline]
    fn is_empty(span: &PayloadRef) -> bool {
        span.start == span.end
    }

    #[inline]
    fn as_slice(&self, span: PayloadRef) -> &[Value] {
        &self.values[span]
    }

    fn take_free_span(&mut self, len: usize) -> Option<PayloadRef> {
        let idx = self.free.iter().position(|r| Self::len(r) >= len)?;
        let span = self.free[idx].clone();

        if Self::len(&span) == len {
            self.free.swap_remove(idx);
            Some(span)
        } else {
            let out = span.start..(span.start + len);
            self.free[idx] = (span.start + len)..span.end;
            Some(out)
        }
    }

    fn reserve_span(&mut self, len: usize) -> PayloadRef {
        if len == 0 {
            return Self::empty();
        }

        let start = match len {
            1 => self.bucket1.pop(),
            2 => self.bucket2.pop(),
            3 => self.bucket3.pop(),
            _ => None,
        };

        if let Some(start) = start {
            return start..(start + len);
        }

        if let Some(span) = self.take_free_span(len) {
            return span;
        }

        let start = self.values.len();
        self.values.reserve(len);
        unsafe {
            self.values.set_len(start + len);
        }
        start..(start + len)
    }

    fn alloc_from_stack_tail(&mut self, stack: &mut Vec<Value>, n: usize) -> PayloadRef {
        if n == 0 {
            return Self::empty();
        }

        let stack_start = stack.len() - n;
        let span = self.reserve_span(n);

        self.values[span.clone()].copy_from_slice(&stack[stack_start..]);
        stack.truncate(stack_start);

        span
    }

    fn alloc_clone(&mut self, src: PayloadRef) -> PayloadRef {
        let len = Self::len(&src);
        if len == 0 {
            return Self::empty();
        }

        let dst = self.reserve_span(len);
        self.values.copy_within(src, dst.start);
        dst
    }

    fn alloc_concat_from_payload_and_stack_tail(
        &mut self,
        prefix: PayloadRef,
        stack: &mut Vec<Value>,
        suffix_len: usize,
    ) -> PayloadRef {
        let prefix_len = Self::len(&prefix);
        let total = prefix_len + suffix_len;

        if total == 0 {
            return Self::empty();
        }

        let dst = self.reserve_span(total);

        if prefix_len > 0 {
            self.values.copy_within(prefix, dst.start);
        }

        if suffix_len > 0 {
            let stack_start = stack.len() - suffix_len;
            let dst_start = dst.start + prefix_len;
            let dst_end = dst_start + suffix_len;

            self.values[dst_start..dst_end].copy_from_slice(&stack[stack_start..]);
            stack.truncate(stack_start);
        }

        dst
    }

    fn free_span(&mut self, span: PayloadRef) {
        let len = Self::len(&span);
        match len {
            0 => {}
            1 => self.bucket1.push(span.start),
            2 => self.bucket2.push(span.start),
            3 => self.bucket3.push(span.start),
            _ => self.free.push(span),
        }
    }
}

#[derive(Debug, Clone)]
enum HeapObject {
    Closure {
        chunk: usize,
        captures: PayloadRef,
    },
    PartialApp {
        chunk: usize,
        captures: PayloadRef,
        supplied: PayloadRef,
    },
    PartialCtor {
        tag: u16,
        arity: u16,
        supplied: PayloadRef,
    },
    Data {
        tag: u16,
        fields: PayloadRef,
    },
}

#[derive(Debug)]
struct Heap {
    objects: Vec<Option<HeapObject>>,
    mark_bits: Vec<bool>,
    free_list: Vec<u32>,
    payloads: PayloadStore,
    allocations_since_gc: usize,
    next_gc_at: usize,
}

impl Default for Heap {
    fn default() -> Self {
        Self {
            objects: Vec::new(),
            mark_bits: Vec::new(),
            free_list: Vec::new(),
            payloads: PayloadStore::default(),
            allocations_since_gc: 0,
            next_gc_at: 32_768,
        }
    }
}

#[derive(Debug, Clone)]
enum HeapTarget {
    Closure {
        chunk: usize,
        captures: PayloadRef,
    },
    PartialApp {
        chunk: usize,
        captures: PayloadRef,
        supplied: PayloadRef,
    },
    PartialCtor {
        tag: u16,
        arity: u16,
        supplied: PayloadRef,
    },
    Data {
        tag: u16,
        fields: PayloadRef,
    },
}

impl Heap {
    #[inline]
    fn get(&self, id: ObjId) -> &HeapObject {
        debug_assert!(
            (id.0 as usize) < self.objects.len() && self.objects[id.0 as usize].is_some(),
            "dangling heap ref {}",
            id.0
        );
        unsafe {
            self.objects
                .get_unchecked(id.0 as usize)
                .as_ref()
                .unwrap_unchecked()
        }
    }

    #[inline]
    fn alloc(&mut self, obj: HeapObject) -> Value {
        self.allocations_since_gc += 1;

        if let Some(id) = self.free_list.pop() {
            let idx = id as usize;
            self.objects[idx] = Some(obj);
            self.mark_bits[idx] = false;
            Value::Obj(ObjId(id))
        } else {
            let id = ObjId(self.objects.len() as u32);
            self.objects.push(Some(obj));
            self.mark_bits.push(false);
            Value::Obj(id)
        }
    }

    fn clone_payload(&mut self, span: PayloadRef) -> PayloadRef {
        self.payloads.alloc_clone(span)
    }

    #[inline]
    fn target(&self, id: ObjId) -> HeapTarget {
        match self.get(id) {
            HeapObject::Closure { chunk, captures } => HeapTarget::Closure {
                chunk: *chunk,
                captures: captures.clone(),
            },
            HeapObject::PartialApp {
                chunk,
                captures,
                supplied,
            } => HeapTarget::PartialApp {
                chunk: *chunk,
                captures: captures.clone(),
                supplied: supplied.clone(),
            },
            HeapObject::PartialCtor {
                tag,
                arity,
                supplied,
            } => HeapTarget::PartialCtor {
                tag: *tag,
                arity: *arity,
                supplied: supplied.clone(),
            },
            HeapObject::Data { tag, fields } => HeapTarget::Data {
                tag: *tag,
                fields: fields.clone(),
            },
        }
    }

    #[inline]
    fn payload_len(&self, span: &PayloadRef) -> usize {
        PayloadStore::len(span)
    }

    #[inline]
    fn copy_payload_to_stack(&self, span: PayloadRef, stack: &mut [Value], dst_start: usize) {
        let src = self.payloads.as_slice(span);
        stack[dst_start..dst_start + src.len()].copy_from_slice(src);
    }

    #[inline]
    fn alloc_payload_from_stack_tail(&mut self, stack: &mut Vec<Value>, n: usize) -> PayloadRef {
        self.payloads.alloc_from_stack_tail(stack, n)
    }

    fn alloc_concat_payload_and_stack_tail(
        &mut self,
        prefix: PayloadRef,
        stack: &mut Vec<Value>,
        suffix_len: usize,
    ) -> PayloadRef {
        self.payloads
            .alloc_concat_from_payload_and_stack_tail(prefix, stack, suffix_len)
    }

    fn free_object_payloads(&mut self, obj: &HeapObject) {
        match obj {
            HeapObject::Closure { captures, .. } => {
                self.payloads.free_span(captures.clone());
            }
            HeapObject::PartialApp {
                captures, supplied, ..
            } => {
                self.payloads.free_span(captures.clone());
                self.payloads.free_span(supplied.clone());
            }
            HeapObject::PartialCtor { supplied, .. } => {
                self.payloads.free_span(supplied.clone());
            }
            HeapObject::Data { fields, .. } => {
                self.payloads.free_span(fields.clone());
            }
        }
    }
}

#[derive(Clone)]
enum GlobalSlot {
    Evaluated(Value),
    Thunk { chunk: usize },
    BlackHole,
}

enum ReturnTo {
    Halt,
    Caller {
        chunk: usize,
        pc: usize,
        frame_base: usize,
    },
    UpdateThenHalt {
        global_index: usize,
    },
    UpdateThenResume {
        global_index: usize,
        chunk: usize,
        pc: usize,
        frame_base: usize,
    },
}

struct BoolCache {
    true_val: Value,
    false_val: Value,
}

struct NullaryCache {
    tags: Vec<Option<Value>>,
}

impl NullaryCache {
    fn new(max_tag: usize) -> Self {
        Self {
            tags: vec![None; max_tag + 1],
        }
    }

    #[inline]
    fn get(&self, tag: u16) -> Value {
        self.tags[tag as usize].expect("missing nullary constructor")
    }

    fn set(&mut self, tag: u16, val: Value) {
        self.tags[tag as usize] = Some(val);
    }
}

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
    for &v in payloads.as_slice(span.clone()) {
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
                Some(HeapObject::Closure { captures, .. }) => Children::One(captures.clone()),
                Some(HeapObject::PartialApp {
                    captures, supplied, ..
                }) => Children::Two(captures.clone(), supplied.clone()),
                Some(HeapObject::PartialCtor { supplied, .. }) => Children::One(supplied.clone()),
                Some(HeapObject::Data { fields, .. }) => Children::One(fields.clone()),
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
    
    #[inline]
    fn push(&mut self, val: Value) {
        self.stack.push(val);
    }

    #[inline]
    fn pop(&mut self) -> Value {
        self.stack.pop().expect("stack underflow")
    }

    #[inline]
    fn pop_int(&mut self) -> i64 {
        match self.pop() {
            Value::Int(n) => n,
            other => panic!("expected Int, got {:?}", other),
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

    #[inline]
    fn load(&self, slot: u16) -> Value {
        self.stack[self.frame_base + slot as usize]
    }

    #[inline]
    fn store(&mut self, slot: u16, val: Value) {
        self.stack[self.frame_base + slot as usize] = val;
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
        capture_source: ObjId,
        arg_count: usize,
    ) {
        let meta = program.chunk(chunk_id);
        let slot_count = meta.slot_count as usize;
        let capture_count = match meta.kind {
            ChunkKind::Function { capture_count, .. } => capture_count as usize,
            _ => panic!("entering thunk as closure"),
        };

        let captures = match self.heap.target(capture_source) {
            HeapTarget::Closure { captures, .. } | HeapTarget::PartialApp { captures, .. } => {
                captures
            }
            _ => panic!("capture source is not a closure"),
        };

        debug_assert_eq!(capture_count, self.heap.payload_len(&captures));

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

    fn tail_enter_known(&mut self, program: &BytecodeProgram, chunk_id: usize, arg_count: usize) {
        let meta = program.chunk(chunk_id);
        let slot_count = meta.slot_count as usize;
        let capture_count = match meta.kind {
            ChunkKind::Function { capture_count, .. } => capture_count as usize,
            _ => panic!("tail-entering thunk as function"),
        };

        if capture_count != 0 {
            panic!(
                "TailCallKnown currently expects zero-capture functions, got {} captures",
                capture_count
            );
        }

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
    }

    fn tail_enter_closure(
        &mut self,
        program: &BytecodeProgram,
        chunk_id: usize,
        capture_source: ObjId,
        arg_count: usize,
    ) {
        let meta = program.chunk(chunk_id);
        let slot_count = meta.slot_count as usize;
        let capture_count = match meta.kind {
            ChunkKind::Function { capture_count, .. } => capture_count as usize,
            _ => panic!("tail-entering thunk as closure"),
        };

        let captures = match self.heap.target(capture_source) {
            HeapTarget::Closure { captures, .. } | HeapTarget::PartialApp { captures, .. } => {
                captures
            }
            _ => panic!("capture source is not a closure"),
        };

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
        match self.heap.target(id) {
            HeapTarget::Closure { chunk, captures } => {
                let arity = chunk_arity(program, chunk);

                if arg_count == arity {
                    self.tail_enter_closure(program, chunk, id, arg_count);
                    None
                } else if arg_count < arity {
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

            HeapTarget::PartialApp {
                chunk,
                captures,
                supplied,
            } => {
                let arity = chunk_arity(program, chunk);
                let total = self.heap.payload_len(&supplied) + arg_count;

                if total == arity {
                    self.tail_enter_with_prefix_and_stack_tail(
                        program, chunk, captures, supplied, arg_count,
                    );
                    None
                } else if total < arity {
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

            HeapTarget::PartialCtor {
                tag,
                arity,
                supplied,
            } => Some(self.build_ctor(tag, arity, supplied, arg_count)),

            HeapTarget::Data { tag, .. } => {
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
        match self.heap.target(id) {
            HeapTarget::Closure { chunk, captures } => {
                let arity = chunk_arity(program, chunk);

                if arg_count == arity {
                    self.push_return();
                    self.enter_closure(program, chunk, id, arg_count);
                    None
                } else if arg_count < arity {
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

            HeapTarget::PartialApp {
                chunk,
                captures,
                supplied,
            } => {
                let arity = chunk_arity(program, chunk);
                let total = self.heap.payload_len(&supplied) + arg_count;

                if total == arity {
                    self.push_return();
                    self.enter_with_prefix_and_stack_tail(
                        program, chunk, captures, supplied, arg_count,
                    );
                    None
                } else if total < arity {
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

            HeapTarget::PartialCtor {
                tag,
                arity,
                supplied,
            } => Some(self.build_ctor(tag, arity, supplied, arg_count)),

            HeapTarget::Data { tag, .. } => {
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
        let matched_arm_idx = {
            let heap = &self.heap;
            let bindings = &mut self.match_buffer;

            let mut matched = None;

            for (arm_idx, arm) in arms.iter().enumerate() {
                bindings.clear();

                if bindings.capacity() < arm.bind_slots.len() {
                    bindings.reserve(arm.bind_slots.len() - bindings.capacity());
                }

                if match_pattern(heap, &arm.pattern, scrutinee, bindings) {
                    matched = Some(arm_idx);
                    break;
                }
            }

            matched
        };

        if let Some(arm_idx) = matched_arm_idx {
            let arm = &arms[arm_idx];

            for i in 0..arm.bind_slots.len() {
                let slot = arm.bind_slots[i];
                let val = self.match_buffer[i];
                self.store(slot, val);
            }

            self.pc = arm.target as usize;
        } else {
            self.pc = else_pc as usize;
        }
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
            // if self.heap.allocations_since_gc >= self.heap.next_gc_at {
            //     self.collect_garbage();
            // }

            let instr = code[self.pc];
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
                        self.push(Value::CtorFn {
                            tag: tag,
                            arity: arity,
                        });
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
                    }
                }

                Instr::Pack {
                    tag,
                    arity,
                    field_count,
                } => {
                    if arity == 0 {
                        self.push(self.nullary.get(tag));
                    } else {
                        self.maybe_gc();
                        let fields = self.pop_payload(field_count as usize);
                        let val = self.alloc_heap(HeapObject::Data { tag: tag, fields });
                        self.push(val);
                    }
                }

                Instr::CallN { arg_count } => {
                    let n = arg_count as usize;
                    let callee_pos = self.stack.len() - n - 1;
                    let callee = self.stack[callee_pos];

                    // Remove the callee from under the args by shifting
                    // args down one slot.  For n=1 (curried language common
                    // case) this is a single copy.
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

                    if let Some(val) = self.tail_apply_callee(program, callee, n) {
                        if let Some(done) = self.finish_return(val) {
                            return done;
                        }
                    }
                    code = &program.chunks[self.chunk].code;
                }

                Instr::TailCallKnown { chunk, arg_count } => {
                    let chunk_id = chunk as usize;
                    let n = arg_count as usize;
                    let args_start = self.stack.len() - n;

                    // Move args to frame base
                    if args_start != self.frame_base {
                        for i in 0..n {
                            self.stack[self.frame_base + i] = self.stack[args_start + i];
                        }
                    }

                    // Truncate and reset
                    self.stack.truncate(self.frame_base + n);
                    self.pc = 0;

                    if chunk_id != self.chunk {
                        self.chunk = chunk_id;
                        let slot_count = program.chunks[chunk_id].slot_count as usize;
                        self.ensure_slots(slot_count);
                        code = &program.chunks[self.chunk].code;
                    } else {
                        // Self-tail-call: chunk and slot_count unchanged, skip ensure_slots
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
                HeapObject::Data { tag, fields } => (*tag, heap.payloads.as_slice(fields.clone())),
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

pub struct DisplayValue<'a> {
    root: Value,
    heap: &'a Heap,
    arities: &'a [(String, u16, u16)],
}

impl fmt::Display for DisplayValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render(self.root))
    }
}

fn base_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

fn tag_name<'a>(arities: &'a [(String, u16, u16)], tag: u16) -> &'a str {
    arities
        .iter()
        .find(|(_, t, _)| *t == tag)
        .map(|(n, _, _)| base_name(n))
        .unwrap_or("<?>")
}

fn tag_is(arities: &[(String, u16, u16)], tag: u16, expected: &str) -> bool {
    tag_name(arities, tag) == expected
}

impl DisplayValue<'_> {
    fn render(&self, val: Value) -> String {
        if let Some(s) = self.try_nat(val) {
            return s;
        }
        if let Some(s) = self.try_list(val) {
            return s;
        }
        if let Some(s) = self.try_pair(val) {
            return s;
        }

        match val {
            Value::Unit => "()".into(),
            Value::Int(n) => n.to_string(),
            Value::CtorFn { tag, .. } => {
                format!("<ctor {}>", tag_name(self.arities, tag))
            }
            Value::Obj(id) => match self.heap.get(id) {
                HeapObject::Closure { .. } | HeapObject::PartialApp { .. } => "<closure>".into(),
                HeapObject::PartialCtor { tag, supplied, .. } => {
                    let name = tag_name(self.arities, *tag);
                    let supplied = self.heap.payloads.as_slice(supplied.clone());

                    if supplied.is_empty() {
                        format!("<partial {}>", name)
                    } else {
                        format!("<partial {} {}>", name, self.render_args(supplied))
                    }
                }
                HeapObject::Data { tag, fields } => {
                    let name = tag_name(self.arities, *tag);
                    let fields = self.heap.payloads.as_slice(fields.clone());

                    if fields.is_empty() {
                        name.into()
                    } else {
                        format!("{} {}", name, self.render_args(fields))
                    }
                }
            },
        }
    }

    fn render_atom(&self, val: Value) -> String {
        let s = self.render(val);
        if s.contains(' ') {
            format!("({})", s)
        } else {
            s
        }
    }

    fn render_args(&self, vals: &[Value]) -> String {
        vals.iter()
            .map(|&v| self.render_atom(v))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn try_nat(&self, mut val: Value) -> Option<String> {
        let mut n = 0usize;

        loop {
            match val {
                Value::Obj(id) => match self.heap.get(id) {
                    HeapObject::Data { tag, fields } => {
                        let fields = self.heap.payloads.as_slice(fields.clone());

                        if fields.is_empty() && tag_is(self.arities, *tag, "Z") {
                            return Some(n.to_string());
                        }

                        if fields.len() == 1 && tag_is(self.arities, *tag, "S") {
                            n += 1;
                            val = fields[0];
                            continue;
                        }

                        return None;
                    }
                    _ => return None,
                },
                _ => return None,
            }
        }
    }

    fn try_list(&self, mut val: Value) -> Option<String> {
        let mut items = Vec::new();

        loop {
            match val {
                Value::Obj(id) => match self.heap.get(id) {
                    HeapObject::Data { tag, fields } => {
                        let fields = self.heap.payloads.as_slice(fields.clone());

                        if fields.is_empty() && tag_is(self.arities, *tag, "Nil") {
                            return Some(format!("[{}]", items.join(", ")));
                        }

                        if fields.len() == 2 && tag_is(self.arities, *tag, "Cons") {
                            items.push(self.render(fields[0]));
                            val = fields[1];
                            continue;
                        }

                        return None;
                    }
                    _ => return None,
                },
                _ => return None,
            }
        }
    }

    fn try_pair(&self, val: Value) -> Option<String> {
        match val {
            Value::Obj(id) => match self.heap.get(id) {
                HeapObject::Data { tag, fields } => {
                    let fields = self.heap.payloads.as_slice(fields.clone());

                    if fields.len() == 2 {
                        let name = tag_name(self.arities, *tag);
                        if name == "Pair" || name == "MkPair" {
                            Some(format!(
                                "({}, {})",
                                self.render(fields[0]),
                                self.render(fields[1])
                            ))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            },
            _ => None,
        }
    }
}
