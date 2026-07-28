//! VM garbage collection marking and heap accounting.
//! Allocation paths use this module to reclaim unreachable runtime objects.

use super::*;
use crate::heap::{HeapObject, IoAction, PayloadRef, PayloadStore};
use crate::values::{GlobalSlot, ObjId, Value};
use solveig_ast::ast::{CtorId, PRIM_TYS};

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
    pub(super) fn init_nullary(&mut self, arities: &[CtorRuntimeInfo]) {
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
    pub(super) fn alloc_heap(&mut self, obj: HeapObject) -> Value {
        self.heap.alloc(obj)
    }

    pub(super) fn maybe_gc(&mut self) {
        if self.heap.allocations_since_gc >= self.heap.next_gc_at {
            self.collect_garbage();
        }
    }

    pub(super) fn collect_garbage(&mut self) {
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
                Some(HeapObject::Data0 { .. }) | Some(HeapObject::Str { .. }) | None => {
                    Children::None
                }
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
}

fn find_ctor_tag(arities: &[CtorRuntimeInfo], ctor: CtorId) -> u16 {
    arities
        .iter()
        .find(|info| info.arity == 0 && info.ctor == ctor)
        .map_or_else(
            || panic!("ICE: runtime constructor metadata missing CtorId {ctor:?}"),
            |info| info.tag,
        )
}
