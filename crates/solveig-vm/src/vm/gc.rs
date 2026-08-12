//! Generational garbage collection and the VM's single root enumeration.
//!
//! `Roots` covers every active and inactive process, evaluated globals,
//! nullary-constructor cache, and Bool cache. Any future field that can hold a
//! `Value` across an allocation point must be added here.

use super::*;
use crate::heap::{ChanId, Heap, HeapObject, IoAction, PayloadRef};
use crate::values::{BoolCache, Continuation, NullaryCtorCache, ObjId, Value};
use solveig_ast::ast::{CtorId, PRIM_TYS};

pub(super) struct Roots<'a> {
    active_process: Option<&'a mut Process>,
    inactive_processes: &'a mut Arena<Process>,
    globals: &'a mut [GlobalSlot],
    nullary_ctors: &'a mut NullaryCtorCache,
    bools: &'a mut BoolCache,
    channel_waiters: &'a mut FxHashMap<ChanId, ChannelWaiters>,
}

impl Roots<'_> {
    fn visit_values_mut(&mut self, mut visit: impl FnMut(&mut Value)) {
        if let Some(process) = self.active_process.as_deref_mut() {
            Self::visit_process_values_mut(process, &mut visit);
        }
        for (_, process) in self.inactive_processes.iter_mut() {
            Self::visit_process_values_mut(process, &mut visit);
        }

        for slot in &mut *self.globals {
            match slot {
                GlobalSlot::Evaluated(value) => visit(value),
                GlobalSlot::Thunk { .. }
                | GlobalSlot::Evaluating { .. }
                | GlobalSlot::Failed(_) => {}
            }
        }
        for value in self.nullary_ctors.tags.iter_mut().flatten() {
            visit(value);
        }
        visit(&mut self.bools.true_val);
        visit(&mut self.bools.false_val);

        let waiters = std::mem::take(self.channel_waiters);
        for (mut channel, mut channel_waiters) in waiters {
            Self::visit_channel_id_mut(&mut channel, &mut visit);
            for (_, value) in &mut channel_waiters.senders {
                visit(value);
            }
            if self
                .channel_waiters
                .insert(channel, channel_waiters)
                .is_some()
            {
                solveig_base::ice!("GC rewrote two channel waiter keys to one identity")
            }
        }
    }

    fn visit_process_values_mut(process: &mut Process, visit: &mut impl FnMut(&mut Value)) {
        process.stack.iter_mut().for_each(&mut *visit);
        process.binding_buffer.iter_mut().for_each(&mut *visit);
        process.temporary_roots.iter_mut().for_each(&mut *visit);
        match &mut process.status {
            ProcStatus::Blocked(BlockReason::Chan(channel)) => {
                Self::visit_channel_id_mut(channel, visit);
            }
            ProcStatus::Finished(value) => visit(value),
            ProcStatus::Runnable
            | ProcStatus::Running
            | ProcStatus::Blocked(
                BlockReason::Io(_) | BlockReason::Blackhole(_) | BlockReason::Timer(_),
            )
            | ProcStatus::Failed(_)
            | ProcStatus::Interrupted(_) => {}
        }
        if let Some(value) = &mut process.io_current {
            visit(value);
        }
        if let Some(ChannelResult::Value(value)) = &mut process.channel_result {
            visit(value);
        }
        for frame in &mut process.continuations {
            match frame {
                Continuation::IoBind(value)
                | Continuation::IoCatch(value)
                | Continuation::IoCatchDefect(value)
                | Continuation::IoFinally(value)
                | Continuation::IoRestore(value)
                | Continuation::IoRestoreDefect(value) => visit(value),
                Continuation::Halt
                | Continuation::CallerFrame { .. }
                | Continuation::PublishGlobalThenHalt { .. }
                | Continuation::PublishGlobalThenResume { .. }
                | Continuation::IoRestoreMask(_)
                | Continuation::IoRestoreInterruption { .. } => {}
            }
        }
    }

    fn visit_channel_id_mut(channel: &mut ChanId, visit: &mut impl FnMut(&mut Value)) {
        let mut value = Value::Obj(channel.0);
        visit(&mut value);
        let Value::Obj(id) = value else {
            solveig_base::ice!("GC changed a channel object root into a non-object value")
        };
        channel.0 = id;
    }

    #[cfg(debug_assertions)]
    fn assert_reachable_live(&mut self, heap: &Heap) {
        let mut seen = vec![false; heap.old.len()];
        let mut worklist = Vec::new();
        self.visit_values_mut(|value| assert_value_live(heap, *value, &mut seen, &mut worklist));

        while let Some(id) = worklist.pop() {
            assert_object_children_live(heap, id, &mut seen, &mut worklist);
        }
    }
}

impl Interpreter<'_> {
    #[inline]
    pub(super) fn alloc_heap(&mut self, obj: HeapObject) -> Value {
        self.heap.alloc(obj)
    }

    /// Reaches the VM's sole suspension boundary and performs pending GC work.
    ///
    /// Following Marlow and Peyton Jones's AOSA GHC runtime chapter, execution
    /// can stop only where the VM stack, registers, and heap roots are tidy.
    pub(crate) fn safe_point(&mut self) {
        if self.reductions_left > 0 {
            self.reductions_left -= 1;
        }
        if self.heap.should_collect() {
            let mut roots = Roots {
                active_process: Some(self.process),
                inactive_processes: self.inactive_processes,
                globals: self.globals,
                nullary_ctors: self.nullary_ctors,
                bools: self.bools,
                channel_waiters: self.channel_waiters,
            };
            collect_garbage(self.heap, &mut roots);
        }
    }

    pub(super) fn collect_for_defect_recovery(&mut self) {
        let mut roots = Roots {
            active_process: Some(self.process),
            inactive_processes: self.inactive_processes,
            globals: self.globals,
            nullary_ctors: self.nullary_ctors,
            bools: self.bools,
            channel_waiters: self.channel_waiters,
        };
        collect_minor(self.heap, &mut roots);
        collect_major(self.heap, &mut roots);

        #[cfg(debug_assertions)]
        roots.assert_reachable_live(self.heap);
    }
}

impl Runtime {
    pub(super) fn init_nullary(&mut self) {
        for index in 0..self.program.constructor_arities.len() {
            let Some(info) = self.program.constructor_arities.get(index) else {
                solveig_base::ice!("constructor metadata index disappeared during initialization");
            };
            let (arity, tag) = (info.arity, info.tag);
            if arity != 0 {
                continue;
            }

            if self.heap.should_collect() {
                let mut roots = Roots {
                    active_process: None,
                    inactive_processes: &mut self.processes,
                    globals: &mut self.globals,
                    nullary_ctors: &mut self.nullary_ctors,
                    bools: &mut self.bools,
                    channel_waiters: &mut self.channel_waiters,
                };
                collect_garbage(&mut self.heap, &mut roots);
            }

            let value = self.heap.alloc(HeapObject::Data0 { tag });
            self.nullary_ctors.set(tag, value);
        }

        let true_tag = find_ctor_tag(&self.program.constructor_arities, PRIM_TYS.true_ctor);
        let false_tag = find_ctor_tag(&self.program.constructor_arities, PRIM_TYS.false_ctor);
        self.bools.true_val = self.nullary_ctors.get(true_tag);
        self.bools.false_val = self.nullary_ctors.get(false_tag);
    }
}

#[cfg(test)]
impl Runtime {
    pub(super) fn alloc_str_for_test(&mut self, value: &str) -> Value {
        if self.heap.should_collect() {
            let mut roots = Roots {
                active_process: None,
                inactive_processes: &mut self.processes,
                globals: &mut self.globals,
                nullary_ctors: &mut self.nullary_ctors,
                bools: &mut self.bools,
                channel_waiters: &mut self.channel_waiters,
            };
            collect_garbage(&mut self.heap, &mut roots);
        }
        self.heap.alloc_str(value)
    }

    pub(super) fn collect_all_process_roots_for_test(&mut self) {
        let mut roots = Roots {
            active_process: None,
            inactive_processes: &mut self.processes,
            globals: &mut self.globals,
            nullary_ctors: &mut self.nullary_ctors,
            bools: &mut self.bools,
            channel_waiters: &mut self.channel_waiters,
        };
        collect_garbage(&mut self.heap, &mut roots);
    }
}

fn collect_garbage(heap: &mut Heap, roots: &mut Roots<'_>) {
    collect_minor(heap, roots);
    if heap.should_collect_major() {
        collect_major(heap, roots);
    }

    #[cfg(debug_assertions)]
    roots.assert_reachable_live(heap);
}

/// Copies nursery survivors using forwarding entries in their vacated slots.
///
/// This is the Cheney-style copying discipline from Jones, Hosking, and Moss,
/// *The Garbage Collection Handbook*, ch. 4, organized as Appel's
/// bump-allocated generational nursery. The explicit worklist keeps traversal
/// off the native Rust stack.
fn collect_minor(heap: &mut Heap, roots: &mut Roots<'_>) {
    let mut worklist = Vec::new();
    let mut promoted = 0usize;

    roots.visit_values_mut(|value| {
        evacuate_value(heap, value, &mut worklist, &mut promoted);
    });

    while let Some(id) = worklist.pop() {
        rewrite_promoted_object(heap, id, &mut worklist, &mut promoted);
    }

    let remembered = std::mem::take(&mut heap.remembered_channels);
    for id in remembered {
        if heap.is_nursery_id(id.0) {
            solveig_base::ice!("remembered set contained a nursery channel")
        }
        let Some(channel) = heap.channel(id) else {
            solveig_base::ice!("remembered channel identity no longer names a channel")
        };
        rewrite_old_payload(heap, channel.queue, &mut worklist, &mut promoted);
    }

    while let Some(id) = worklist.pop() {
        rewrite_promoted_object(heap, id, &mut worklist, &mut promoted);
    }

    heap.finish_minor_collection(promoted);
}

fn evacuate_value(
    heap: &mut Heap,
    value: &mut Value,
    worklist: &mut Vec<ObjId>,
    promoted: &mut usize,
) {
    let Value::Obj(id) = value else {
        return;
    };
    if !heap.is_nursery_id(*id) {
        return;
    }

    let nursery_index = id.0 as usize;
    let object = *heap
        .nursery
        .get(nursery_index)
        .unwrap_or_else(|| solveig_base::ice!("GC encountered dangling nursery ref {}", id.0));
    if let HeapObject::Forwarded(destination) = object {
        *id = destination;
        return;
    }

    let promoted_object = copy_object_side_payloads(heap, object);
    let destination = heap.alloc_old(promoted_object);
    *heap
        .nursery
        .get_mut(nursery_index)
        .expect("ICE: evacuated nursery slot disappeared") = HeapObject::Forwarded(destination);
    *id = destination;
    worklist.push(destination);
    *promoted += 1;
}

fn copy_object_side_payloads(heap: &mut Heap, object: HeapObject) -> HeapObject {
    match object {
        HeapObject::Closure { chunk, captures } => HeapObject::Closure {
            chunk,
            captures: heap.promote_payload(captures),
        },
        HeapObject::PartialApp {
            chunk,
            captures,
            applied_args,
        } => HeapObject::PartialApp {
            chunk,
            captures: heap.promote_payload(captures),
            applied_args: heap.promote_payload(applied_args),
        },
        HeapObject::PartialCtor {
            tag,
            arity,
            applied_args,
        } => HeapObject::PartialCtor {
            tag,
            arity,
            applied_args: heap.promote_payload(applied_args),
        },
        HeapObject::DataN { tag, fields } => HeapObject::DataN {
            tag,
            fields: heap.promote_payload(fields),
        },
        HeapObject::Str { bytes } => HeapObject::Str {
            bytes: heap.promote_bytes(bytes),
        },
        HeapObject::Chan(mut channel) => {
            channel.queue = heap.promote_payload(channel.queue);
            HeapObject::Chan(channel)
        }
        HeapObject::Data0 { .. }
        | HeapObject::Data1 { .. }
        | HeapObject::Data2 { .. }
        | HeapObject::IoAction(_) => object,
        HeapObject::Forwarded(_) => {
            solveig_base::ice!("forwarding entry copied as a nursery survivor");
        }
    }
}

fn rewrite_promoted_object(
    heap: &mut Heap,
    id: ObjId,
    worklist: &mut Vec<ObjId>,
    promoted: &mut usize,
) {
    let old_index = heap.old_index(id);
    let Some(mut object) = heap.old.get(old_index).and_then(|slot| *slot) else {
        solveig_base::ice!("promoted object {} disappeared before scanning", id.0);
    };

    match &mut object {
        HeapObject::Closure { captures, .. }
        | HeapObject::PartialCtor {
            applied_args: captures,
            ..
        }
        | HeapObject::DataN {
            fields: captures, ..
        } => rewrite_old_payload(heap, *captures, worklist, promoted),
        HeapObject::PartialApp {
            captures,
            applied_args,
            ..
        } => {
            rewrite_old_payload(heap, *captures, worklist, promoted);
            rewrite_old_payload(heap, *applied_args, worklist, promoted);
        }
        HeapObject::Data1 { field, .. } => {
            evacuate_value(heap, field, worklist, promoted);
        }
        HeapObject::Data2 { field0, field1, .. } => {
            evacuate_value(heap, field0, worklist, promoted);
            evacuate_value(heap, field1, worklist, promoted);
        }
        HeapObject::IoAction(action) => {
            rewrite_io_action(heap, action, worklist, promoted);
        }
        HeapObject::Chan(channel) => {
            rewrite_old_payload(heap, channel.queue, worklist, promoted);
        }
        HeapObject::Data0 { .. } | HeapObject::Str { .. } => {}
        HeapObject::Forwarded(_) => {
            solveig_base::ice!("forwarding entry reached promoted-object scan");
        }
    }

    *heap
        .old
        .get_mut(old_index)
        .expect("ICE: promoted old object slot disappeared") = Some(object);
}

fn rewrite_old_payload(
    heap: &mut Heap,
    span: PayloadRef,
    worklist: &mut Vec<ObjId>,
    promoted: &mut usize,
) {
    for index in span.start..span.end {
        let mut value = heap.payloads.old_value(index);
        evacuate_value(heap, &mut value, worklist, promoted);
        heap.payloads.set_old_value(index, value);
    }
}

fn rewrite_io_action(
    heap: &mut Heap,
    action: &mut IoAction,
    worklist: &mut Vec<ObjId>,
    promoted: &mut usize,
) {
    match action {
        IoAction::Pure(value)
        | IoAction::Fail(value)
        | IoAction::RaiseDefect { defect: value }
        | IoAction::Print { text: value, .. }
        | IoAction::Spawn { action: value }
        | IoAction::KillThread { thread: value }
        | IoAction::Mask { action: value }
        | IoAction::RestoreMask { action: value } => {
            evacuate_value(heap, value, worklist, promoted);
        }
        IoAction::Bind {
            action,
            continuation,
        }
        | IoAction::Catch {
            action,
            handler: continuation,
        }
        | IoAction::CatchDefect {
            action,
            handler: continuation,
        }
        | IoAction::Finally {
            action,
            cleanup: continuation,
        } => {
            evacuate_value(heap, action, worklist, promoted);
            evacuate_value(heap, continuation, worklist, promoted);
        }
        IoAction::ReadFile { path } => {
            evacuate_value(heap, path, worklist, promoted);
        }
        IoAction::WriteFile { path, contents, .. } => {
            evacuate_value(heap, path, worklist, promoted);
            evacuate_value(heap, contents, worklist, promoted);
        }
        IoAction::Send { channel, value } => {
            evacuate_value(heap, channel, worklist, promoted);
            evacuate_value(heap, value, worklist, promoted);
        }
        IoAction::Recv { channel }
        | IoAction::TryRecv { channel }
        | IoAction::CloseChan { channel } => {
            evacuate_value(heap, channel, worklist, promoted);
        }
        IoAction::ReadLine
        | IoAction::GetArgs
        | IoAction::NewChan { .. }
        | IoAction::Sleep { .. } => {}
    }
}

/// Reuses the pre-I4 mark-sweep collector for non-moving old-generation slots.
fn collect_major(heap: &mut Heap, roots: &mut Roots<'_>) {
    let nursery_slot_count = heap.nursery_slot_count;
    let mut worklist = Vec::new();
    roots.visit_values_mut(|value| {
        mark_old_value(
            nursery_slot_count,
            &mut heap.mark_bits,
            &heap.old,
            *value,
            &mut worklist,
        );
    });

    while let Some(id) = worklist.pop() {
        mark_old_object_children(heap, id, &mut worklist);
    }

    heap.prepare_major_collection();
    let mut live = 0usize;
    for index in 0..heap.old.len() {
        let dead = {
            let marked = heap
                .mark_bits
                .get_mut(index)
                .expect("ICE: major GC mark-bit index out of bounds");
            if *marked {
                *marked = false;
                live += 1;
                None
            } else {
                heap.old
                    .get_mut(index)
                    .expect("ICE: major GC object index out of bounds")
                    .take()
            }
        };

        if let Some(dead) = dead {
            heap.free_old_object_payloads(&dead);
        }
        if heap.old.get(index).is_some_and(Option::is_none) {
            heap.free_list.push(
                u32::try_from(index)
                    .unwrap_or_else(|_| solveig_base::ice!("old free-list index overflow")),
            );
        }
    }
    heap.finish_major_collection(live);
}

fn mark_old_value(
    nursery_slot_count: usize,
    mark_bits: &mut [bool],
    old: &[Option<HeapObject>],
    value: Value,
    worklist: &mut Vec<ObjId>,
) {
    let Value::Obj(id) = value else {
        return;
    };
    let Some(index) = (id.0 as usize).checked_sub(nursery_slot_count) else {
        solveig_base::ice!("nursery reference survived minor collection: {}", id.0);
    };
    if !old.get(index).is_some_and(Option::is_some) {
        solveig_base::ice!("major GC root references freed object {}", id.0);
    }
    let marked = mark_bits
        .get_mut(index)
        .expect("ICE: major GC mark index out of bounds");
    if !*marked {
        *marked = true;
        worklist.push(id);
    }
}

fn mark_old_object_children(heap: &mut Heap, id: ObjId, worklist: &mut Vec<ObjId>) {
    let object = *heap.get(id);
    match object {
        HeapObject::Closure { captures, .. }
        | HeapObject::PartialCtor {
            applied_args: captures,
            ..
        }
        | HeapObject::DataN {
            fields: captures, ..
        } => mark_old_payload(heap, captures, worklist),
        HeapObject::PartialApp {
            captures,
            applied_args,
            ..
        } => {
            mark_old_payload(heap, captures, worklist);
            mark_old_payload(heap, applied_args, worklist);
        }
        HeapObject::Data1 { field, .. } => mark_old_child(heap, field, worklist),
        HeapObject::Data2 { field0, field1, .. } => {
            mark_old_child(heap, field0, worklist);
            mark_old_child(heap, field1, worklist);
        }
        HeapObject::IoAction(action) => mark_old_io_action(heap, action, worklist),
        HeapObject::Chan(channel) => mark_old_payload(heap, channel.queue, worklist),
        HeapObject::Data0 { .. } | HeapObject::Str { .. } => {}
        HeapObject::Forwarded(_) => {
            solveig_base::ice!("forwarding entry reached old-generation marking");
        }
    }
}

fn mark_old_payload(heap: &mut Heap, span: PayloadRef, worklist: &mut Vec<ObjId>) {
    for index in span.start..span.end {
        let value = heap.payloads.old_value(index);
        mark_old_child(heap, value, worklist);
    }
}

fn mark_old_child(heap: &mut Heap, value: Value, worklist: &mut Vec<ObjId>) {
    mark_old_value(
        heap.nursery_slot_count,
        &mut heap.mark_bits,
        &heap.old,
        value,
        worklist,
    );
}

fn mark_old_io_action(heap: &mut Heap, action: IoAction, worklist: &mut Vec<ObjId>) {
    match action {
        IoAction::Pure(value)
        | IoAction::Fail(value)
        | IoAction::RaiseDefect { defect: value }
        | IoAction::Print { text: value, .. }
        | IoAction::Spawn { action: value }
        | IoAction::KillThread { thread: value }
        | IoAction::Mask { action: value }
        | IoAction::RestoreMask { action: value } => {
            mark_old_child(heap, value, worklist);
        }
        IoAction::Bind {
            action,
            continuation,
        }
        | IoAction::Catch {
            action,
            handler: continuation,
        }
        | IoAction::CatchDefect {
            action,
            handler: continuation,
        }
        | IoAction::Finally {
            action,
            cleanup: continuation,
        } => {
            mark_old_child(heap, action, worklist);
            mark_old_child(heap, continuation, worklist);
        }
        IoAction::ReadFile { path } => mark_old_child(heap, path, worklist),
        IoAction::WriteFile { path, contents, .. } => {
            mark_old_child(heap, path, worklist);
            mark_old_child(heap, contents, worklist);
        }
        IoAction::Send { channel, value } => {
            mark_old_child(heap, channel, worklist);
            mark_old_child(heap, value, worklist);
        }
        IoAction::Recv { channel }
        | IoAction::TryRecv { channel }
        | IoAction::CloseChan { channel } => mark_old_child(heap, channel, worklist),
        IoAction::ReadLine
        | IoAction::GetArgs
        | IoAction::NewChan { .. }
        | IoAction::Sleep { .. } => {}
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
    if !heap.is_live(id) {
        solveig_base::ice!("GC root set references freed object {}", id.0);
    }
    if heap.is_nursery_id(id) {
        solveig_base::ice!(
            "GC root set retains nursery object {} after collection",
            id.0
        );
    }

    let seen_slot = seen
        .get_mut(heap.old_index(id))
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
    match *heap.get(id) {
        HeapObject::Closure { captures, .. }
        | HeapObject::PartialCtor {
            applied_args: captures,
            ..
        }
        | HeapObject::DataN {
            fields: captures, ..
        } => assert_payload_live(heap, id, captures, seen, worklist),
        HeapObject::PartialApp {
            captures,
            applied_args,
            ..
        } => {
            assert_payload_live(heap, id, captures, seen, worklist);
            assert_payload_live(heap, id, applied_args, seen, worklist);
        }
        HeapObject::Data1 { field, .. } => assert_value_live(heap, field, seen, worklist),
        HeapObject::Data2 { field0, field1, .. } => {
            assert_value_live(heap, field0, seen, worklist);
            assert_value_live(heap, field1, seen, worklist);
        }
        HeapObject::IoAction(action) => match action {
            IoAction::Pure(value)
            | IoAction::Fail(value)
            | IoAction::RaiseDefect { defect: value }
            | IoAction::Print { text: value, .. }
            | IoAction::Spawn { action: value }
            | IoAction::KillThread { thread: value }
            | IoAction::Mask { action: value }
            | IoAction::RestoreMask { action: value } => {
                assert_value_live(heap, value, seen, worklist);
            }
            IoAction::Bind {
                action,
                continuation,
            }
            | IoAction::Catch {
                action,
                handler: continuation,
            }
            | IoAction::CatchDefect {
                action,
                handler: continuation,
            }
            | IoAction::Finally {
                action,
                cleanup: continuation,
            } => {
                assert_value_live(heap, action, seen, worklist);
                assert_value_live(heap, continuation, seen, worklist);
            }
            IoAction::ReadFile { path } => assert_value_live(heap, path, seen, worklist),
            IoAction::WriteFile { path, contents, .. } => {
                assert_value_live(heap, path, seen, worklist);
                assert_value_live(heap, contents, seen, worklist);
            }
            IoAction::Send { channel, value } => {
                assert_value_live(heap, channel, seen, worklist);
                assert_value_live(heap, value, seen, worklist);
            }
            IoAction::Recv { channel }
            | IoAction::TryRecv { channel }
            | IoAction::CloseChan { channel } => {
                assert_value_live(heap, channel, seen, worklist);
            }
            IoAction::ReadLine
            | IoAction::GetArgs
            | IoAction::NewChan { .. }
            | IoAction::Sleep { .. } => {}
        },
        HeapObject::Chan(channel) => {
            assert_payload_live(heap, id, channel.queue, seen, worklist);
        }
        HeapObject::Data0 { .. } | HeapObject::Str { .. } => {}
        HeapObject::Forwarded(_) => {
            solveig_base::ice!("forwarding entry reached debug heap walk");
        }
    }
}

#[cfg(debug_assertions)]
fn assert_payload_live(
    heap: &Heap,
    owner: ObjId,
    span: PayloadRef,
    seen: &mut [bool],
    worklist: &mut Vec<ObjId>,
) {
    for &value in heap.payload_slice(owner, span) {
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
