//! Bytecode instruction dispatch loop.
//! This module mutates VM stack, frames, heap, and IO state for each instruction.

use super::*;
use crate::builtins;
use crate::bytecode::{BytecodeProgram, Instr, MatchArm};
use crate::heap::{Heap, HeapObject};
use crate::values::Value;
use solveig_ast::ast::Literal;
use solveig_lower::LowerPattern;

impl Interpreter<'_> {
    pub(super) fn dispatch_match(
        &mut self,
        program: &BytecodeProgram,
        arms: &[MatchArm],
        scrutinee: Value,
        else_pc: u32,
    ) {
        for arm in arms {
            match &arm.pattern {
                LowerPattern::Var => {
                    self.store(bind_slot(arm, 0), scrutinee);
                    self.pc = arm.target as usize;
                    return;
                }
                LowerPattern::Lit(_) => {
                    self.binding_buffer.clear();

                    if match_pattern(
                        program,
                        &*self.heap,
                        &arm.pattern,
                        scrutinee,
                        &mut self.process.binding_buffer,
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
                                    self.store(bind_slot(arm, 0), field);
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
                            let [sub0, sub1] = subs.as_slice() else {
                                solveig_base::ice!("Data2 pattern length was checked above");
                            };

                            if let Some(_bind_count) = try_flat_bind(subs) {
                                let mut si = 0;

                                if matches!(sub0, LowerPattern::Var) {
                                    self.store(bind_slot(arm, si), f0);
                                    si += 1;
                                }

                                if matches!(sub1, LowerPattern::Var) {
                                    self.store(bind_slot(arm, si), f1);
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

                                self.process.binding_buffer.clear();
                                for (sub, &field) in subs.iter().zip(fields.iter()) {
                                    if matches!(sub, LowerPattern::Var) {
                                        self.process.binding_buffer.push(field);
                                    }
                                }

                                debug_assert_eq!(self.binding_buffer.len(), bind_count);
                                for (index, &slot) in
                                    arm.bind_slots.iter().enumerate().take(bind_count)
                                {
                                    let value = *self
                                        .binding_buffer
                                        .get(index)
                                        .expect("ICE: flat pattern binding index out of bounds");
                                    self.store(slot, value);
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
                        &*self.heap,
                        &arm.pattern,
                        scrutinee,
                        &mut self.process.binding_buffer,
                    ) {
                        for (index, &slot) in arm.bind_slots.iter().enumerate() {
                            let value = *self
                                .binding_buffer
                                .get(index)
                                .expect("ICE: pattern binding index out of bounds");
                            self.store(slot, value);
                        }
                        self.pc = arm.target as usize;
                        return;
                    }
                }
            }
        }

        self.pc = else_pc as usize;
    }

    pub(super) fn dispatch(&mut self, program: &BytecodeProgram) -> Result<Value, RuntimeError> {
        let mut code = &program.chunk(self.chunk).code;
        loop {
            // SAFETY: The bytecode emitter and jump patching only produce PCs
            // inside the current chunk; any violation is an internal bytecode ICE.
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
                            solveig_base::ice!(
                                "bytecode references missing string constant {index}"
                            )
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
                    code = &program.chunk(self.chunk).code;
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

                    code = &program.chunk(self.chunk).code;
                }

                Instr::CallKnown { chunk, arg_count } => {
                    let chunk_id = chunk as usize;
                    let n = arg_count as usize;

                    if let Some(val) =
                        self.invoke_known_function(program, InvokeMode::Call, chunk_id, n)
                    {
                        self.push(val);
                    }

                    code = &program.chunk(self.chunk).code;
                }
                Instr::TailCallN { arg_count } => {
                    let n = arg_count as usize;
                    let callee = self.take_callee_under_args(n);

                    if let Some(val) = self.invoke_value(program, InvokeMode::TailCall, callee, n)
                        && let Some(done) = self.resume_after_return(val)
                    {
                        return Ok(done);
                    }

                    code = &program.chunk(self.chunk).code;
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

                    code = &program.chunk(self.chunk).code;
                }
                Instr::Return => {
                    let result = self.pop();

                    if let Some(done) = self.resume_after_return(result) {
                        return Ok(done);
                    }

                    code = &program.chunk(self.chunk).code;
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
                    let table = program
                        .chunk(self.chunk)
                        .match_tables
                        .get(table_idx as usize)
                        .expect("ICE: bytecode match instruction references missing table");
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

#[inline]
fn bind_slot(arm: &MatchArm, index: usize) -> u16 {
    *arm.bind_slots
        .get(index)
        .expect("ICE: pattern bind slot index out of bounds")
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
                    let Some(sub) = subs.first() else {
                        solveig_base::ice!("Data1 pattern length was checked above");
                    };
                    match_pattern_inner(program, heap, sub, *field, bindings)
                }
                HeapObject::Data2 {
                    tag,
                    field0,
                    field1,
                } => {
                    if *tag != *expected_tag || subs.len() != 2 {
                        return false;
                    }
                    let [sub0, sub1] = subs.as_slice() else {
                        solveig_base::ice!("Data2 pattern length was checked above");
                    };
                    let cp = bindings.len();
                    if !match_pattern_inner(program, heap, sub0, *field0, bindings) {
                        bindings.truncate(cp);
                        return false;
                    }
                    if !match_pattern_inner(program, heap, sub1, *field1, bindings) {
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
