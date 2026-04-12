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

#[derive(Debug, Clone)]
enum HeapObject {
    Closure {
        chunk: usize,
        captures: Box<[Value]>,
    },
    PartialApp {
        chunk: usize,
        captures: Box<[Value]>,
        supplied: Box<[Value]>,
    },
    PartialCtor {
        tag: u16,
        arity: u16,
        supplied: Box<[Value]>,
    },
    Data {
        tag: u16,
        fields: Box<[Value]>,
    },
}

#[derive(Debug, Default)]
struct Heap {
    objects: Vec<HeapObject>,
}

impl Heap {
    #[inline]
    fn alloc(&mut self, obj: HeapObject) -> Value {
        let id = ObjId(self.objects.len() as u32);
        self.objects.push(obj);
        Value::Obj(id)
    }

    #[inline]
    fn get(&self, id: ObjId) -> &HeapObject {
        &self.objects[id.0 as usize]
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

    heap: Heap,
    globals: Vec<GlobalSlot>,
    nullary: NullaryCache,
    bools: BoolCache,
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
                let val = self.heap.alloc(HeapObject::Data {
                    tag,
                    fields: Box::new([]),
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
    fn pop_box(&mut self, n: usize) -> Box<[Value]> {
        let start = self.stack.len() - n;
        let result: Box<[Value]> = self.stack[start..].into();
        self.stack.truncate(start);
        result
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

    /// Inline global forcing during execution: either pushes the value
    /// or enters the thunk with an update continuation.
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

    // ── Function entry ──────────────────────────────────────────

    fn enter_thunk(&mut self, program: &BytecodeProgram, chunk_id: usize) {
        let meta = program.chunk(chunk_id);
        self.chunk = chunk_id;
        self.pc = 0;
        self.frame_base = self.stack.len();
        self.stack
            .resize(self.frame_base + meta.slot_count as usize, Value::Unit);
    }

    /// Enter a function.  `arg_count` arguments are on top of the stack.
    /// Captures are read from the heap object at `capture_source`.
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

        let args_start = self.stack.len() - arg_count;
        self.chunk = chunk_id;
        self.pc = 0;
        self.frame_base = args_start;
        self.ensure_slots(slot_count);

        // Shift args right to make room for captures at the front of the frame.
        if capture_count > 0 {
            for i in (0..arg_count).rev() {
                self.stack[self.frame_base + capture_count + i] = self.stack[self.frame_base + i];
            }
            // Write captures — disjoint field access: heap (read) vs stack (write).
            match &self.heap.objects[capture_source.0 as usize] {
                HeapObject::Closure { captures, .. } | HeapObject::PartialApp { captures, .. } => {
                    for (i, &cap) in captures.iter().enumerate() {
                        self.stack[self.frame_base + i] = cap;
                    }
                }
                _ => panic!("capture source is not a closure"),
            }
        }
    }

    fn enter_with_args(
        &mut self,
        program: &BytecodeProgram,
        chunk_id: usize,
        captures: &[Value],
        args: &[Value],
    ) {
        let meta = program.chunk(chunk_id);
        let slot_count = meta.slot_count as usize;

        self.chunk = chunk_id;
        self.pc = 0;
        self.frame_base = self.stack.len();
        self.ensure_slots(slot_count);

        for (i, &cap) in captures.iter().enumerate() {
            self.stack[self.frame_base + i] = cap;
        }
        let offset = captures.len();
        for (i, &arg) in args.iter().enumerate() {
            self.stack[self.frame_base + offset + i] = arg;
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

            match &self.heap.objects[capture_source.0 as usize] {
                HeapObject::Closure { captures, .. } | HeapObject::PartialApp { captures, .. } => {
                    for (i, &cap) in captures.iter().enumerate() {
                        self.stack[self.frame_base + i] = cap;
                    }
                }
                _ => panic!("capture source is not a closure"),
            }
        }
    }

    fn tail_enter_with_args(
        &mut self,
        program: &BytecodeProgram,
        chunk_id: usize,
        captures: &[Value],
        args: &[Value],
    ) {
        let meta = program.chunk(chunk_id);
        let slot_count = meta.slot_count as usize;

        self.stack.truncate(self.frame_base);
        self.chunk = chunk_id;
        self.pc = 0;
        self.ensure_slots(slot_count);

        for (i, &cap) in captures.iter().enumerate() {
            self.stack[self.frame_base + i] = cap;
        }

        let offset = captures.len();
        for (i, &arg) in args.iter().enumerate() {
            self.stack[self.frame_base + offset + i] = arg;
        }
    }

    fn tail_apply_callee(
        &mut self,
        program: &BytecodeProgram,
        callee: Value,
        arg_count: usize,
    ) -> Option<Value> {
        match callee {
            Value::CtorFn { tag, arity } => Some(self.build_ctor(tag, arity, &[], arg_count)),

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
        enum Target {
            Closure {
                chunk: usize,
            },
            Partial {
                chunk: usize,
                n_supplied: usize,
            },
            Ctor {
                tag: u16,
                arity: u16,
                n_supplied: usize,
            },
            Saturated {
                tag: u16,
            },
        }

        let target = match self.heap.get(id) {
            HeapObject::Closure { chunk, .. } => Target::Closure { chunk: *chunk },
            HeapObject::PartialApp {
                chunk, supplied, ..
            } => Target::Partial {
                chunk: *chunk,
                n_supplied: supplied.len(),
            },
            HeapObject::PartialCtor {
                tag,
                arity,
                supplied,
            } => Target::Ctor {
                tag: *tag,
                arity: *arity,
                n_supplied: supplied.len(),
            },
            HeapObject::Data { tag, .. } => Target::Saturated { tag: *tag },
        };

        match target {
            Target::Closure { chunk } => {
                let arity = chunk_arity(program, chunk);

                if arg_count == arity {
                    self.tail_enter_closure(program, chunk, id, arg_count);
                    None
                } else if arg_count < arity {
                    let new_args = self.pop_box(arg_count);
                    let captures = self.clone_captures(id);

                    Some(self.heap.alloc(HeapObject::PartialApp {
                        chunk,
                        captures,
                        supplied: new_args,
                    }))
                } else {
                    panic!(
                        "over-application of closure (arity {}, got {})",
                        arity, arg_count
                    );
                }
            }

            Target::Partial { chunk, n_supplied } => {
                let arity = chunk_arity(program, chunk);
                let total = n_supplied + arg_count;

                if total == arity {
                    let new_args = self.pop_box(arg_count);
                    let captures = self.clone_captures(id);
                    let supplied = self.clone_supplied(id);

                    let mut all_args = Vec::with_capacity(total);
                    all_args.extend_from_slice(&supplied);
                    all_args.extend_from_slice(&new_args);

                    self.tail_enter_with_args(program, chunk, &captures, &all_args);
                    None
                } else if total < arity {
                    let new_args = self.pop_box(arg_count);
                    let captures = self.clone_captures(id);
                    let supplied = self.clone_supplied(id);

                    let mut all = Vec::with_capacity(total);
                    all.extend_from_slice(&supplied);
                    all.extend_from_slice(&new_args);

                    Some(self.heap.alloc(HeapObject::PartialApp {
                        chunk,
                        captures,
                        supplied: all.into_boxed_slice(),
                    }))
                } else {
                    panic!(
                        "over-application of partial (arity {}, got {})",
                        arity, total
                    );
                }
            }

            Target::Ctor {
                tag,
                arity,
                n_supplied,
            } => {
                let supplied = self.clone_supplied(id);
                debug_assert_eq!(n_supplied, supplied.len());
                Some(self.build_ctor(tag, arity, &supplied, arg_count))
            }

            Target::Saturated { tag } => {
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
        match callee {
            Value::CtorFn { tag, arity } => Some(self.build_ctor(tag, arity, &[], arg_count)),

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
        enum Target {
            Closure {
                chunk: usize,
            },
            Partial {
                chunk: usize,
                n_supplied: usize,
            },
            Ctor {
                tag: u16,
                arity: u16,
                n_supplied: usize,
            },
            Saturated {
                tag: u16,
            },
        }

        let target = match self.heap.get(id) {
            HeapObject::Closure { chunk, .. } => Target::Closure { chunk: *chunk },
            HeapObject::PartialApp {
                chunk, supplied, ..
            } => Target::Partial {
                chunk: *chunk,
                n_supplied: supplied.len(),
            },
            HeapObject::PartialCtor {
                tag,
                arity,
                supplied,
            } => Target::Ctor {
                tag: *tag,
                arity: *arity,
                n_supplied: supplied.len(),
            },
            HeapObject::Data { tag, .. } => Target::Saturated { tag: *tag },
        };
        match target {
            Target::Closure { chunk } => {
                let arity = chunk_arity(program, chunk);
                if arg_count == arity {
                    self.push_return();
                    self.enter_closure(program, chunk, id, arg_count);
                    None
                } else if arg_count < arity {
                    let new_args = self.pop_box(arg_count);
                    let captures = self.clone_captures(id);
                    Some(self.heap.alloc(HeapObject::PartialApp {
                        chunk,
                        captures,
                        supplied: new_args,
                    }))
                } else {
                    panic!(
                        "over-application of closure (arity {}, got {})",
                        arity, arg_count
                    );
                }
            }

            Target::Partial { chunk, n_supplied } => {
                let arity = chunk_arity(program, chunk);
                let total = n_supplied + arg_count;

                if total == arity {
                    let new_args = self.pop_box(arg_count);
                    let captures = self.clone_captures(id);
                    let supplied = self.clone_supplied(id);

                    let mut all_args = Vec::with_capacity(total);
                    all_args.extend_from_slice(&supplied);
                    all_args.extend_from_slice(&new_args);

                    self.push_return();
                    self.enter_with_args(program, chunk, &captures, &all_args);
                    None
                } else if total < arity {
                    let new_args = self.pop_box(arg_count);
                    let captures = self.clone_captures(id);
                    let supplied = self.clone_supplied(id);

                    let mut all = Vec::with_capacity(total);
                    all.extend_from_slice(&supplied);
                    all.extend_from_slice(&new_args);

                    Some(self.heap.alloc(HeapObject::PartialApp {
                        chunk,
                        captures,
                        supplied: all.into_boxed_slice(),
                    }))
                } else {
                    panic!(
                        "over-application of partial (arity {}, got {})",
                        arity, total
                    );
                }
            }

            Target::Ctor {
                tag,
                arity,
                n_supplied,
            } => {
                let supplied = self.clone_supplied(id);
                Some(self.build_ctor(tag, arity, &supplied, arg_count))
            }

            Target::Saturated { tag } => {
                panic!("cannot apply saturated constructor tag {}", tag);
            }
        }
    }

    fn build_ctor(&mut self, tag: u16, arity: u16, existing: &[Value], n_new: usize) -> Value {
        let new_fields = self.pop_box(n_new);
        let total = existing.len() + new_fields.len();

        let mut all = Vec::with_capacity(total);
        all.extend_from_slice(existing);
        all.extend_from_slice(&new_fields);

        if total == arity as usize {
            self.heap.alloc(HeapObject::Data {
                tag,
                fields: all.into_boxed_slice(),
            })
        } else if total < arity as usize {
            self.heap.alloc(HeapObject::PartialCtor {
                tag,
                arity,
                supplied: all.into_boxed_slice(),
            })
        } else {
            panic!("over-applied constructor tag {}", tag);
        }
    }

    fn clone_captures(&self, id: ObjId) -> Box<[Value]> {
        match self.heap.get(id) {
            HeapObject::Closure { captures, .. } | HeapObject::PartialApp { captures, .. } => {
                captures.clone()
            }
            _ => panic!("expected closure/partial"),
        }
    }

    fn clone_supplied(&self, id: ObjId) -> Box<[Value]> {
        match self.heap.get(id) {
            HeapObject::PartialApp { supplied, .. } | HeapObject::PartialCtor { supplied, .. } => {
                supplied.clone()
            }
            _ => panic!("expected partial app/ctor"),
        }
    }

    fn dispatch_match(&mut self, arms: &[MatchArm], scrutinee: Value, else_pc: u32) {
        let mut bindings = Vec::new();
        for arm in arms {
            bindings.clear();
            if self.match_pattern(&arm.pattern, scrutinee, &mut bindings) {
                for (&slot, &val) in arm.bind_slots.iter().zip(bindings.iter()) {
                    self.store(slot, val);
                }
                self.pc = arm.target as usize;
                return;
            }
        }
        self.pc = else_pc as usize;
    }

    fn match_pattern(
        &self,
        pattern: &BytePattern,
        value: Value,
        bindings: &mut Vec<Value>,
    ) -> bool {
        let mut work = vec![(pattern, value)];
        while let Some((pat, val)) = work.pop() {
            match pat {
                BytePattern::Var => bindings.push(val),
                BytePattern::Wild => {}
                BytePattern::Con(expected_tag, subs) => {
                    let id = match val {
                        Value::Obj(id) => id,
                        _ => return false,
                    };
                    match self.heap.get(id) {
                        HeapObject::Data { tag, fields }
                            if *tag == *expected_tag && fields.len() == subs.len() =>
                        {
                            for (sub, &field) in subs.iter().zip(fields.iter()).rev() {
                                work.push((sub, field));
                            }
                        }
                        _ => return false,
                    }
                }
            }
        }
        true
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
        loop {
            let instr = &program.chunk(self.chunk).code[self.pc];
            self.pc += 1;

            match instr {
                Instr::PushUnit => self.push(Value::Unit),
                Instr::PushInt(n) => self.push(Value::Int(*n)),

                Instr::LoadLocal(slot) => self.push(self.load(*slot)),
                Instr::StoreLocal(slot) => {
                    let val = self.pop();
                    self.store(*slot, val);
                }

                Instr::LoadGlobal(index) => {
                    self.force_global_inline(program, *index as usize);
                }

                Instr::PushCtorFn { tag, arity } => {
                    if *arity == 0 {
                        self.push(self.nullary.get(*tag));
                    } else {
                        self.push(Value::CtorFn {
                            tag: *tag,
                            arity: *arity,
                        });
                    }
                }

                Instr::MakeClosure {
                    chunk,
                    capture_count,
                } => {
                    let captures = self.pop_box(*capture_count as usize);
                    let val = self.heap.alloc(HeapObject::Closure {
                        chunk: *chunk as usize,
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
                    if *arity == 0 {
                        self.push(self.nullary.get(*tag));
                    } else {
                        let fields = self.pop_box(*field_count as usize);
                        let val = self.heap.alloc(HeapObject::Data { tag: *tag, fields });
                        self.push(val);
                    }
                }

                Instr::CallN { arg_count } => {
                    let n = *arg_count as usize;
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
                }

                Instr::CallKnown { chunk, arg_count } => {
                    let chunk_id = *chunk as usize;
                    let n = *arg_count as usize;

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
                }
                Instr::TailCallN { arg_count } => {
                    let n = *arg_count as usize;
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
                }

                Instr::TailCallKnown { chunk, arg_count } => {
                    let chunk_id = *chunk as usize;
                    let n = *arg_count as usize;
                    self.tail_enter_known(program, chunk_id, n);
                }
                Instr::Return => {
                    let result = self.pop();
                    if let Some(done) = self.finish_return(result) {
                        return done;
                    }
                }
                Instr::Jump(target) => self.pc = *target as usize,
                Instr::JumpIfFalse(target) => {
                    let cond = self.pop();
                    if cond == self.bools.false_val {
                        self.pc = *target as usize;
                    }
                }
                Instr::Match { arms, else_target } => {
                    let scrutinee = self.pop();
                    self.dispatch_match(arms, scrutinee, *else_target);
                }
                Instr::Trap(msg) => panic!("{}", msg),
            }
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
                    if supplied.is_empty() {
                        format!("<partial {}>", name)
                    } else {
                        format!("<partial {} {}>", name, self.render_args(supplied))
                    }
                }
                HeapObject::Data { tag, fields } => {
                    let name = tag_name(self.arities, *tag);
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
                    HeapObject::Data { tag, fields }
                        if fields.is_empty() && tag_is(self.arities, *tag, "Z") =>
                    {
                        return Some(n.to_string());
                    }
                    HeapObject::Data { tag, fields }
                        if fields.len() == 1 && tag_is(self.arities, *tag, "S") =>
                    {
                        n += 1;
                        val = fields[0];
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
                    HeapObject::Data { tag, fields }
                        if fields.is_empty() && tag_is(self.arities, *tag, "Nil") =>
                    {
                        return Some(format!("[{}]", items.join(", ")));
                    }
                    HeapObject::Data { tag, fields }
                        if fields.len() == 2 && tag_is(self.arities, *tag, "Cons") =>
                    {
                        items.push(self.render(fields[0]));
                        val = fields[1];
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
                HeapObject::Data { tag, fields } if fields.len() == 2 => {
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
                }
                _ => None,
            },
            _ => None,
        }
    }
}
