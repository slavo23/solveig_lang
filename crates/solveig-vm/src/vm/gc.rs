//! VM garbage collection marking and heap accounting.
//! Allocation paths use this module to reclaim unreachable runtime objects.
//!
//! `Roots` is the single root-set definition for the current VM: the value
//! stack, pattern binding buffer, short-lived temporary roots, evaluated
//! globals, nullary-constructor cache, Bool cache, current IO action, and IO
//! continuation stack. Any future VM field that can hold a `Value` across an
//! allocation point must be added to `Roots`.

use super::*;
use crate::heap::{Heap, HeapObject, IoAction, PayloadRef, PayloadStore};
use crate::values::{BoolCache, GlobalSlot, IoFrame, NullaryCtorCache, ObjId, Value};
use solveig_ast::ast::{CtorId, PRIM_TYS};

#[inline]
fn mark_value(mark_bits: &mut [bool], value: Value, worklist: &mut Vec<ObjId>) {
    if let Value::Obj(id) = value {
        mark_bit(mark_bits, id, worklist);
    }
}

#[inline]
fn mark_bit(mark_bits: &mut [bool], id: ObjId, worklist: &mut Vec<ObjId>) {
    let idx = id.0 as usize;
    if let Some(marked) = mark_bits.get_mut(idx)
        && !*marked
    {
        *marked = true;
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

pub(super) struct Roots<'a> {
    stack: &'a [Value],
    binding_buffer: &'a [Value],
    temporary_roots: &'a [Value],
    io_current: Option<Value>,
    io_stack: &'a [IoFrame],
    globals: &'a [GlobalSlot],
    nullary_ctors: &'a NullaryCtorCache,
    bools: &'a BoolCache,
}

impl<'a> Roots<'a> {
    fn mark(&self, mark_bits: &mut [bool], worklist: &mut Vec<ObjId>) {
        self.mark_values(mark_bits, self.stack, worklist);
        self.mark_values(mark_bits, self.binding_buffer, worklist);
        self.mark_values(mark_bits, self.temporary_roots, worklist);
        self.mark_io(mark_bits, worklist);

        for slot in self.globals {
            if let GlobalSlot::Evaluated(value) = slot {
                mark_value(mark_bits, *value, worklist);
            }
        }

        for value in self.nullary_ctors.tags.iter().flatten() {
            mark_value(mark_bits, *value, worklist);
        }

        mark_value(mark_bits, self.bools.true_val, worklist);
        mark_value(mark_bits, self.bools.false_val, worklist);
    }

    fn mark_io(&self, mark_bits: &mut [bool], worklist: &mut Vec<ObjId>) {
        if let Some(value) = self.io_current {
            mark_value(mark_bits, value, worklist);
        }

        for frame in self.io_stack {
            match frame {
                IoFrame::Bind(continuation) => mark_value(mark_bits, *continuation, worklist),
            }
        }
    }

    fn mark_values(&self, mark_bits: &mut [bool], values: &[Value], worklist: &mut Vec<ObjId>) {
        for &value in values {
            mark_value(mark_bits, value, worklist);
        }
    }

    #[cfg(debug_assertions)]
    fn assert_reachable_live(&self, heap: &Heap) {
        let mut seen = vec![false; heap.objects.len()];
        let mut worklist = Vec::new();

        self.assert_values_live(heap, self.stack, &mut seen, &mut worklist);
        self.assert_values_live(heap, self.binding_buffer, &mut seen, &mut worklist);
        self.assert_values_live(heap, self.temporary_roots, &mut seen, &mut worklist);
        self.assert_io_live(heap, &mut seen, &mut worklist);

        for slot in self.globals {
            if let GlobalSlot::Evaluated(value) = slot {
                assert_value_live(heap, *value, &mut seen, &mut worklist);
            }
        }

        for value in self.nullary_ctors.tags.iter().flatten() {
            assert_value_live(heap, *value, &mut seen, &mut worklist);
        }

        assert_value_live(heap, self.bools.true_val, &mut seen, &mut worklist);
        assert_value_live(heap, self.bools.false_val, &mut seen, &mut worklist);

        while let Some(id) = worklist.pop() {
            assert_object_children_live(heap, id, &mut seen, &mut worklist);
        }
    }

    #[cfg(debug_assertions)]
    fn assert_io_live(&self, heap: &Heap, seen: &mut [bool], worklist: &mut Vec<ObjId>) {
        if let Some(value) = self.io_current {
            assert_value_live(heap, value, seen, worklist);
        }

        for frame in self.io_stack {
            match frame {
                IoFrame::Bind(continuation) => {
                    assert_value_live(heap, *continuation, seen, worklist);
                }
            }
        }
    }

    #[cfg(debug_assertions)]
    fn assert_values_live(
        &self,
        heap: &Heap,
        values: &[Value],
        seen: &mut [bool],
        worklist: &mut Vec<ObjId>,
    ) {
        for &value in values {
            assert_value_live(heap, value, seen, worklist);
        }
    }
}

impl Vm {
    pub(super) fn init_nullary(&mut self, arities: &[CtorRuntimeInfo]) {
        for info in arities {
            if info.arity == 0 {
                self.maybe_gc();
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
        // AOSA's GHC RTS chapter treats allocation checks as safe points; the
        // stress knob makes that check fail at a low fixed allocation threshold.
        if self.heap.should_collect() {
            self.collect_garbage();
        }
    }

    pub(super) fn collect_garbage(&mut self) {
        let mut worklist: Vec<ObjId> = Vec::new();
        let roots = Roots {
            stack: &self.stack,
            binding_buffer: &self.binding_buffer,
            temporary_roots: &self.temporary_roots,
            io_current: self.io_current,
            io_stack: &self.io_stack,
            globals: &self.globals,
            nullary_ctors: &self.nullary_ctors,
            bools: &self.bools,
        };

        roots.mark(&mut self.heap.mark_bits, &mut worklist);

        while let Some(id) = worklist.pop() {
            mark_object_children(
                &self.heap.objects,
                &self.heap.payloads,
                &mut self.heap.mark_bits,
                id,
                &mut worklist,
            );
        }

        self.heap.payloads.bucket1.clear();
        self.heap.payloads.bucket2.clear();
        self.heap.payloads.bucket3.clear();
        self.heap.payloads.free.clear();
        self.heap.free_list.clear();

        let mut live = 0usize;
        for idx in 0..self.heap.objects.len() {
            let dead = {
                let Some(object_slot) = self.heap.objects.get_mut(idx) else {
                    solveig_base::ice!("GC sweep object index out of bounds");
                };
                if object_slot.is_none() {
                    None
                } else {
                    let marked = self
                        .heap
                        .mark_bits
                        .get_mut(idx)
                        .expect("ICE: GC mark bit index out of bounds");
                    if *marked {
                        *marked = false;
                        live += 1;
                        None
                    } else {
                        object_slot.take()
                    }
                }
            };

            if let Some(dead) = dead {
                self.heap.free_object_payloads(&dead);
                self.heap.free_list.push(
                    u32::try_from(idx)
                        .unwrap_or_else(|_| solveig_base::ice!("heap free-list index overflow")),
                );
            }
        }

        #[cfg(debug_assertions)]
        {
            let roots = Roots {
                stack: &self.stack,
                binding_buffer: &self.binding_buffer,
                temporary_roots: &self.temporary_roots,
                io_current: self.io_current,
                io_stack: &self.io_stack,
                globals: &self.globals,
                nullary_ctors: &self.nullary_ctors,
                bools: &self.bools,
            };
            roots.assert_reachable_live(&self.heap);
        }

        self.heap.reset_collection_accounting(live);
    }
}

enum Children {
    None,
    One(PayloadRef),
    Two(PayloadRef, PayloadRef),
}

fn mark_object_children(
    objects: &[Option<HeapObject>],
    payloads: &PayloadStore,
    mark_bits: &mut [bool],
    id: ObjId,
    worklist: &mut Vec<ObjId>,
) {
    let children = match objects.get(id.0 as usize).and_then(Option::as_ref) {
        Some(HeapObject::Closure { captures, .. }) => Children::One(*captures),
        Some(HeapObject::PartialApp {
            captures,
            applied_args,
            ..
        }) => Children::Two(*captures, *applied_args),
        Some(HeapObject::PartialCtor { applied_args, .. }) => Children::One(*applied_args),
        Some(HeapObject::DataN { fields, .. }) => Children::One(*fields),
        Some(HeapObject::Data0 { .. }) | Some(HeapObject::Str { .. }) | None => Children::None,
        Some(HeapObject::Data1 { field, .. }) => {
            mark_value(mark_bits, *field, worklist);
            Children::None
        }
        Some(HeapObject::Data2 { field0, field1, .. }) => {
            mark_value(mark_bits, *field0, worklist);
            mark_value(mark_bits, *field1, worklist);
            Children::None
        }
        Some(HeapObject::IoAction(action)) => {
            match action {
                IoAction::Pure(value) | IoAction::Print { text: value, .. } => {
                    mark_value(mark_bits, *value, worklist);
                }
                IoAction::Bind {
                    action,
                    continuation,
                } => {
                    mark_value(mark_bits, *action, worklist);
                    mark_value(mark_bits, *continuation, worklist);
                }
                IoAction::ReadLine | IoAction::GetArgs => {}
                IoAction::ReadFile { path } => {
                    mark_value(mark_bits, *path, worklist);
                }
                IoAction::WriteFile { path, contents, .. } => {
                    mark_value(mark_bits, *path, worklist);
                    mark_value(mark_bits, *contents, worklist);
                }
            }
            Children::None
        }
    };

    match children {
        Children::None => {}
        Children::One(span) => mark_payload(payloads, mark_bits, &span, worklist),
        Children::Two(a, b) => {
            mark_payload(payloads, mark_bits, &a, worklist);
            mark_payload(payloads, mark_bits, &b, worklist);
        }
    }
}

#[cfg(debug_assertions)]
fn assert_value_live(heap: &Heap, value: Value, seen: &mut [bool], worklist: &mut Vec<ObjId>) {
    if let Value::Obj(id) = value {
        assert_object_live(heap, id, seen, worklist);
    }
}

#[cfg(debug_assertions)]
fn assert_object_live(heap: &Heap, id: ObjId, seen: &mut [bool], worklist: &mut Vec<ObjId>) {
    let idx = id.0 as usize;
    if !heap.objects.get(idx).is_some_and(Option::is_some) {
        solveig_base::ice!("GC root set references freed object {}", id.0);
    }

    let seen_slot = seen
        .get_mut(idx)
        .expect("ICE: GC seen index out of bounds for live object");
    if !*seen_slot {
        *seen_slot = true;
        worklist.push(id);
    }
}

#[cfg(debug_assertions)]
fn assert_object_children_live(
    heap: &Heap,
    id: ObjId,
    seen: &mut [bool],
    worklist: &mut Vec<ObjId>,
) {
    let Some(object) = heap.objects.get(id.0 as usize).and_then(|slot| *slot) else {
        solveig_base::ice!("GC reachable object {} was freed", id.0);
    };

    match object {
        HeapObject::Closure { captures, .. } => {
            assert_payload_live(heap, captures, seen, worklist);
        }
        HeapObject::PartialApp {
            captures,
            applied_args,
            ..
        } => {
            assert_payload_live(heap, captures, seen, worklist);
            assert_payload_live(heap, applied_args, seen, worklist);
        }
        HeapObject::PartialCtor { applied_args, .. } => {
            assert_payload_live(heap, applied_args, seen, worklist);
        }
        HeapObject::DataN { fields, .. } => {
            assert_payload_live(heap, fields, seen, worklist);
        }
        HeapObject::Data1 { field, .. } => {
            assert_value_live(heap, field, seen, worklist);
        }
        HeapObject::Data2 { field0, field1, .. } => {
            assert_value_live(heap, field0, seen, worklist);
            assert_value_live(heap, field1, seen, worklist);
        }
        HeapObject::IoAction(action) => match action {
            IoAction::Pure(value) | IoAction::Print { text: value, .. } => {
                assert_value_live(heap, value, seen, worklist);
            }
            IoAction::Bind {
                action,
                continuation,
            } => {
                assert_value_live(heap, action, seen, worklist);
                assert_value_live(heap, continuation, seen, worklist);
            }
            IoAction::ReadFile { path } => {
                assert_value_live(heap, path, seen, worklist);
            }
            IoAction::WriteFile { path, contents, .. } => {
                assert_value_live(heap, path, seen, worklist);
                assert_value_live(heap, contents, seen, worklist);
            }
            IoAction::ReadLine | IoAction::GetArgs => {}
        },
        HeapObject::Data0 { .. } | HeapObject::Str { .. } => {}
    }
}

#[cfg(debug_assertions)]
fn assert_payload_live(
    heap: &Heap,
    span: PayloadRef,
    seen: &mut [bool],
    worklist: &mut Vec<ObjId>,
) {
    for &value in heap.payloads.as_slice(span) {
        assert_value_live(heap, value, seen, worklist);
    }
}

fn find_ctor_tag(arities: &[CtorRuntimeInfo], ctor: CtorId) -> u16 {
    arities
        .iter()
        .find(|info| info.arity == 0 && info.ctor == ctor)
        .map_or_else(
            || solveig_base::ice!("runtime constructor metadata missing CtorId {ctor:?}"),
            |info| info.tag,
        )
}
