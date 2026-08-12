//! Managed heap storage for VM values, closures, thunks, and constructors.
//! Garbage collection and runtime allocation share this ownership boundary.

use crate::values::{ObjId, Value};
use solveig_base::FxHashSet;

const DEFAULT_NURSERY_SLOTS: usize = 32_768;
const DEFAULT_MAJOR_LIVE_FLOOR: usize = 65_536;
const DEFAULT_GC_STRESS_SLOTS: usize = 256;

#[derive(Debug, Clone, Copy)]
pub enum HeapObject {
    Closure {
        chunk: usize,
        captures: PayloadRef,
    },
    PartialApp {
        chunk: usize,
        captures: PayloadRef,
        applied_args: PayloadRef,
    },
    PartialCtor {
        tag: u16,
        arity: u16,
        applied_args: PayloadRef,
    },
    Data0 {
        tag: u16,
    },
    Data1 {
        tag: u16,
        field: Value,
    },
    Data2 {
        tag: u16,
        field0: Value,
        field1: Value,
    },
    DataN {
        tag: u16,
        fields: PayloadRef,
    },
    Str {
        bytes: ByteRef,
    },
    Chan(Chan),
    IoAction(IoAction),
    /// A nursery slot that has been evacuated during the current minor GC.
    Forwarded(ObjId),
}

const _: () = assert!(std::mem::size_of::<HeapObject>() == 48);

#[derive(Debug, Clone, Copy)]
pub enum IoAction {
    Pure(Value),
    Fail(Value),
    Bind {
        action: Value,
        continuation: Value,
    },
    Catch {
        action: Value,
        handler: Value,
    },
    CatchDefect {
        action: Value,
        handler: Value,
    },
    Finally {
        action: Value,
        cleanup: Value,
    },
    Print {
        text: Value,
        newline: bool,
    },
    ReadLine,
    GetArgs,
    Spawn {
        action: Value,
    },
    NewChan {
        capacity: Option<i64>,
    },
    Send {
        channel: Value,
        value: Value,
    },
    Recv {
        channel: Value,
    },
    TryRecv {
        channel: Value,
    },
    CloseChan {
        channel: Value,
    },
    ReadFile {
        path: Value,
    },
    WriteFile {
        path: Value,
        contents: Value,
        append: bool,
    },
}

/// Identifies a channel heap object while it is registered with the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChanId(pub ObjId);

/// Stores the mutable heap-owned state of one typed channel.
#[derive(Debug, Clone, Copy)]
pub struct Chan {
    pub queue: PayloadRef,
    pub capacity: Option<u32>,
    pub closed: bool,
}

#[derive(Debug)]
pub struct Heap {
    pub(crate) nursery: Vec<HeapObject>,
    pub(crate) old: Vec<Option<HeapObject>>,
    pub(crate) mark_bits: Vec<bool>,
    pub(crate) free_list: Vec<u32>,
    pub(crate) payloads: PayloadStore,
    pub(crate) bytes: ByteStore,
    pub(crate) nursery_slot_count: usize,
    pub(crate) allocation_limit: usize,
    pub(crate) promoted_since_major: usize,
    pub(crate) next_major_at: usize,
    pub(crate) gc_stress: bool,
    pub(crate) minor_collections: usize,
    pub(crate) major_collections: usize,
    pub(crate) remembered_channels: FxHashSet<ChanId>,
}

impl Default for Heap {
    fn default() -> Self {
        let gc_stress = gc_stress_enabled();
        let gc_threshold_override = gc_threshold_override();
        let nursery_slot_count = initial_collection_threshold(gc_stress, gc_threshold_override);

        Self {
            nursery: Vec::with_capacity(nursery_slot_count),
            old: Vec::new(),
            mark_bits: Vec::new(),
            free_list: Vec::new(),
            payloads: PayloadStore::default(),
            bytes: ByteStore::default(),
            nursery_slot_count,
            allocation_limit: nursery_slot_count,
            promoted_since_major: 0,
            next_major_at: initial_major_threshold(gc_stress),
            gc_stress,
            minor_collections: 0,
            major_collections: 0,
            remembered_channels: FxHashSet::default(),
        }
    }
}

impl Heap {
    #[inline]
    pub fn should_collect(&self) -> bool {
        self.nursery.len() >= self.allocation_limit
    }

    #[inline]
    pub(crate) fn should_collect_major(&self) -> bool {
        self.promoted_since_major >= self.next_major_at
    }

    pub(crate) fn live_slot_count(&self) -> usize {
        self.nursery.len() + self.old.len() - self.free_list.len()
    }

    #[inline]
    pub(crate) fn is_nursery_id(&self, id: ObjId) -> bool {
        (id.0 as usize) < self.nursery_slot_count
    }

    #[inline]
    pub(crate) fn old_index(&self, id: ObjId) -> usize {
        (id.0 as usize)
            .checked_sub(self.nursery_slot_count)
            .unwrap_or_else(|| solveig_base::ice!("nursery object id used as an old object id"))
    }

    #[inline]
    pub fn get(&self, id: ObjId) -> &HeapObject {
        let object = if self.is_nursery_id(id) {
            self.nursery
                .get(id.0 as usize)
                .unwrap_or_else(|| solveig_base::ice!("dangling nursery heap ref {}", id.0))
        } else {
            self.old
                .get(self.old_index(id))
                .and_then(Option::as_ref)
                .unwrap_or_else(|| solveig_base::ice!("dangling old-generation heap ref {}", id.0))
        };

        if matches!(object, HeapObject::Forwarded(_)) {
            solveig_base::ice!(
                "forwarding entry escaped minor collection for object {}",
                id.0
            );
        }
        object
    }

    fn get_mut(&mut self, id: ObjId) -> &mut HeapObject {
        if self.is_nursery_id(id) {
            self.nursery
                .get_mut(id.0 as usize)
                .unwrap_or_else(|| solveig_base::ice!("dangling nursery heap ref {}", id.0))
        } else {
            let index = self.old_index(id);
            self.old
                .get_mut(index)
                .and_then(Option::as_mut)
                .unwrap_or_else(|| solveig_base::ice!("dangling old-generation heap ref {}", id.0))
        }
    }

    #[inline]
    pub fn alloc(&mut self, obj: HeapObject) -> Value {
        if self.nursery.len() >= self.nursery_slot_count {
            solveig_base::ice!("nursery allocation bypassed its allocation-limit safe point");
        }
        if matches!(obj, HeapObject::Forwarded(_)) {
            solveig_base::ice!("normal allocation attempted to publish a forwarding entry");
        }

        let id = ObjId(
            u32::try_from(self.nursery.len())
                .unwrap_or_else(|_| solveig_base::ice!("nursery object id overflow")),
        );
        self.nursery.push(obj);
        Value::Obj(id)
    }

    pub(crate) fn alloc_old(&mut self, obj: HeapObject) -> ObjId {
        if matches!(obj, HeapObject::Forwarded(_)) {
            solveig_base::ice!("old generation cannot contain a forwarding entry");
        }

        let old_index = if let Some(index) = self.free_list.pop() {
            let index = index as usize;
            *self
                .old
                .get_mut(index)
                .expect("ICE: old-generation free-list index out of bounds") = Some(obj);
            *self
                .mark_bits
                .get_mut(index)
                .expect("ICE: old-generation mark-bit index out of bounds") = false;
            index
        } else {
            let index = self.old.len();
            self.old.push(Some(obj));
            self.mark_bits.push(false);
            index
        };

        let raw = self
            .nursery_slot_count
            .checked_add(old_index)
            .unwrap_or_else(|| solveig_base::ice!("old-generation object id overflow"));
        ObjId(
            u32::try_from(raw)
                .unwrap_or_else(|_| solveig_base::ice!("old-generation object id overflow")),
        )
    }

    pub(crate) fn channel(&self, id: ChanId) -> Option<Chan> {
        match self.get(id.0) {
            HeapObject::Chan(channel) => Some(*channel),
            HeapObject::Closure { .. }
            | HeapObject::PartialApp { .. }
            | HeapObject::PartialCtor { .. }
            | HeapObject::Data0 { .. }
            | HeapObject::Data1 { .. }
            | HeapObject::Data2 { .. }
            | HeapObject::DataN { .. }
            | HeapObject::Str { .. }
            | HeapObject::IoAction(_) => None,
            HeapObject::Forwarded(_) => {
                solveig_base::ice!("forwarding entry escaped minor collection")
            }
        }
    }

    /// Appends to a channel queue and records any old-to-young edge.
    ///
    /// The remembered-set rule follows Jones, Hosking, and Moss, *The Garbage
    /// Collection Handbook*, chapter 9: every mutation that installs a nursery
    /// reference in an old object must publish that old object to the collector.
    pub(crate) fn push_channel(&mut self, id: ChanId, value: Value) {
        let Some(mut channel) = self.channel(id) else {
            solveig_base::ice!("validated channel identity no longer names a channel")
        };
        let owner_is_nursery = self.is_nursery_id(id.0);
        let mut values = self
            .payloads
            .as_slice(owner_is_nursery, channel.queue)
            .to_vec();
        values.push(value);

        let next = if owner_is_nursery {
            self.payloads.alloc_nursery_from_values(&values)
        } else {
            let next = self.payloads.alloc_old_from_values(&values);
            self.payloads.free_old_span(channel.queue);
            if matches!(value, Value::Obj(child) if self.is_nursery_id(child)) {
                self.remembered_channels.insert(id);
            }
            next
        };
        channel.queue = next;
        *self.get_mut(id.0) = HeapObject::Chan(channel);
    }

    pub(crate) fn pop_channel(&mut self, id: ChanId) -> Option<Value> {
        let mut channel = self.channel(id)?;
        let owner_is_nursery = self.is_nursery_id(id.0);
        let value = self
            .payloads
            .as_slice(owner_is_nursery, channel.queue)
            .first()
            .copied()?;
        let consumed = PayloadRef {
            start: channel.queue.start,
            end: channel.queue.start + 1,
        };
        channel.queue.start += 1;
        if channel.queue.is_empty() {
            channel.queue = PayloadStore::empty();
        }
        if !owner_is_nursery {
            self.payloads.free_old_span(consumed);
        }
        *self.get_mut(id.0) = HeapObject::Chan(channel);
        Some(value)
    }

    pub(crate) fn close_channel(&mut self, id: ChanId) {
        let Some(mut channel) = self.channel(id) else {
            solveig_base::ice!("validated channel identity no longer names a channel")
        };
        channel.closed = true;
        *self.get_mut(id.0) = HeapObject::Chan(channel);
    }

    #[inline(always)]
    pub fn alloc_data2(&mut self, tag: u16, a: Value, b: Value) -> Value {
        self.alloc(HeapObject::Data2 {
            tag,
            field0: a,
            field1: b,
        })
    }

    pub fn clone_payload(&mut self, owner: ObjId, span: PayloadRef) -> PayloadRef {
        let owner_is_nursery = self.is_nursery_id(owner);
        self.payloads.alloc_nursery_clone(owner_is_nursery, span)
    }

    #[inline]
    pub fn payload_len(&self, span: &PayloadRef) -> usize {
        span.len()
    }

    #[inline]
    pub fn payload_slice(&self, owner: ObjId, span: PayloadRef) -> &[Value] {
        self.payloads.as_slice(self.is_nursery_id(owner), span)
    }

    #[inline]
    pub fn copy_payload_to_stack(
        &self,
        owner: ObjId,
        span: PayloadRef,
        stack: &mut [Value],
        dst_start: usize,
    ) {
        let src = self.payload_slice(owner, span);
        let dst_end = dst_start + src.len();
        stack
            .get_mut(dst_start..dst_end)
            .expect("ICE: stack payload destination out of bounds")
            .copy_from_slice(src);
    }

    #[inline]
    pub fn alloc_payload_from_stack_tail(
        &mut self,
        stack: &mut Vec<Value>,
        n: usize,
    ) -> PayloadRef {
        self.payloads.alloc_nursery_from_stack_tail(stack, n)
    }

    pub fn alloc_concat_payload_and_stack_tail(
        &mut self,
        owner: Option<ObjId>,
        prefix: PayloadRef,
        stack: &mut Vec<Value>,
        suffix_len: usize,
    ) -> PayloadRef {
        let owner_is_nursery = owner.map(|id| self.is_nursery_id(id));
        self.payloads
            .alloc_nursery_concat(owner_is_nursery, prefix, stack, suffix_len)
    }

    pub(crate) fn promote_payload(&mut self, span: PayloadRef) -> PayloadRef {
        self.payloads.copy_nursery_to_old(span)
    }

    pub(crate) fn promote_bytes(&mut self, span: ByteRef) -> ByteRef {
        self.bytes.copy_nursery_to_old(span)
    }

    pub(crate) fn free_old_object_payloads(&mut self, obj: &HeapObject) {
        match obj {
            HeapObject::Closure { captures, .. } => self.payloads.free_old_span(*captures),
            HeapObject::PartialApp {
                captures,
                applied_args,
                ..
            } => {
                self.payloads.free_old_span(*captures);
                self.payloads.free_old_span(*applied_args);
            }
            HeapObject::PartialCtor { applied_args, .. } => {
                self.payloads.free_old_span(*applied_args);
            }
            HeapObject::DataN { fields, .. } => self.payloads.free_old_span(*fields),
            HeapObject::Chan(channel) => self.payloads.free_old_span(channel.queue),
            HeapObject::Str { bytes } => self.bytes.free_old_span(*bytes),
            HeapObject::Data0 { .. }
            | HeapObject::Data1 { .. }
            | HeapObject::Data2 { .. }
            | HeapObject::IoAction(_) => {}
            HeapObject::Forwarded(_) => {
                solveig_base::ice!("forwarding entry reached old-generation payload freeing");
            }
        }
    }

    pub fn alloc_str(&mut self, value: &str) -> Value {
        let bytes = self.bytes.alloc_nursery(value.as_bytes());
        self.alloc(HeapObject::Str { bytes })
    }

    pub fn str_value(&self, owner: ObjId, bytes: ByteRef) -> &str {
        self.bytes.as_str(self.is_nursery_id(owner), bytes)
    }

    pub(crate) fn finish_minor_collection(&mut self, promoted: usize) {
        self.nursery.clear();
        self.payloads.clear_nursery();
        self.bytes.clear_nursery();
        self.allocation_limit = self.nursery_slot_count;
        self.promoted_since_major = self
            .promoted_since_major
            .checked_add(promoted)
            .unwrap_or_else(|| solveig_base::ice!("minor collection promotion count overflow"));
        self.minor_collections += 1;
    }

    pub(crate) fn prepare_major_collection(&mut self) {
        self.free_list.clear();
    }

    pub(crate) fn finish_major_collection(&mut self, live: usize) {
        self.promoted_since_major = 0;
        self.next_major_at = next_major_threshold_after_collection(self.gc_stress, live);
        self.major_collections += 1;
    }

    #[cfg(debug_assertions)]
    pub(crate) fn is_live(&self, id: ObjId) -> bool {
        if self.is_nursery_id(id) {
            self.nursery
                .get(id.0 as usize)
                .is_some_and(|object| !matches!(object, HeapObject::Forwarded(_)))
        } else {
            self.old
                .get(self.old_index(id))
                .is_some_and(Option::is_some)
        }
    }

    #[cfg(test)]
    pub(crate) fn set_collection_limits_for_test(
        &mut self,
        nursery_slot_count: usize,
        next_major_at: usize,
    ) {
        if !self.nursery.is_empty() || !self.old.is_empty() {
            solveig_base::ice!("test collection limits changed after heap allocation");
        }
        self.nursery_slot_count = nursery_slot_count;
        self.allocation_limit = nursery_slot_count;
        self.next_major_at = next_major_at;
        self.nursery = Vec::with_capacity(nursery_slot_count);
    }
}

fn gc_stress_enabled() -> bool {
    std::env::var("SOLVEIG_GC_STRESS").is_ok_and(|value| {
        value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("yes")
    })
}

fn gc_threshold_override() -> Option<usize> {
    std::env::var("SOLVEIG_GC_THRESHOLD")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|threshold| *threshold > 0)
}

fn initial_collection_threshold(gc_stress: bool, gc_threshold_override: Option<usize>) -> usize {
    gc_threshold_override.unwrap_or(if gc_stress {
        DEFAULT_GC_STRESS_SLOTS
    } else {
        DEFAULT_NURSERY_SLOTS
    })
}

fn initial_major_threshold(gc_stress: bool) -> usize {
    if gc_stress {
        DEFAULT_GC_STRESS_SLOTS
    } else {
        DEFAULT_NURSERY_SLOTS
    }
}

fn next_major_threshold_after_collection(gc_stress: bool, live: usize) -> usize {
    if gc_stress {
        DEFAULT_GC_STRESS_SLOTS
    } else {
        live.max(DEFAULT_MAJOR_LIVE_FLOOR) * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gc_stress_uses_low_fixed_threshold_by_default() {
        assert_eq!(
            initial_collection_threshold(true, None),
            DEFAULT_GC_STRESS_SLOTS
        );
        assert_eq!(
            next_major_threshold_after_collection(true, 100_000),
            DEFAULT_GC_STRESS_SLOTS
        );
    }

    #[test]
    fn explicit_gc_threshold_overrides_the_nursery_only() {
        assert_eq!(initial_collection_threshold(true, Some(1)), 1);
        assert_eq!(initial_collection_threshold(false, Some(512)), 512);
        assert_eq!(initial_major_threshold(true), DEFAULT_GC_STRESS_SLOTS);
        assert_eq!(initial_major_threshold(false), DEFAULT_NURSERY_SLOTS);
        assert_eq!(
            next_major_threshold_after_collection(false, 100_000),
            400_000
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadRef {
    pub start: usize,
    pub end: usize,
}

impl PayloadRef {
    #[inline(always)]
    pub fn len(self) -> usize {
        self.end - self.start
    }

    #[inline(always)]
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundPrefix {
    pub owner: Option<ObjId>,
    pub captures: PayloadRef,
    pub applied_args: PayloadRef,
}

impl BoundPrefix {
    pub const EMPTY: Self = Self {
        owner: None,
        captures: PayloadRef { start: 0, end: 0 },
        applied_args: PayloadRef { start: 0, end: 0 },
    };

    #[inline(always)]
    pub fn total_len(self) -> usize {
        self.captures.len() + self.applied_args.len()
    }

    #[inline(always)]
    pub fn is_empty(self) -> bool {
        self.captures.is_empty() && self.applied_args.is_empty()
    }
}

#[derive(Debug, Default)]
pub struct PayloadStore {
    nursery_values: Vec<Value>,
    old_values: Vec<Value>,
    old_free: Vec<PayloadRef>,
    old_bucket1: Vec<usize>,
    old_bucket2: Vec<usize>,
    old_bucket3: Vec<usize>,
}

impl PayloadStore {
    #[inline]
    pub fn empty() -> PayloadRef {
        PayloadRef { start: 0, end: 0 }
    }

    #[inline]
    pub(crate) fn as_slice(&self, nursery: bool, span: PayloadRef) -> &[Value] {
        let values = if nursery {
            &self.nursery_values
        } else {
            &self.old_values
        };
        values
            .get(span.start..span.end)
            .expect("ICE: payload store span out of bounds")
    }

    #[inline]
    pub(crate) fn old_value(&self, index: usize) -> Value {
        *self
            .old_values
            .get(index)
            .expect("ICE: old payload value index out of bounds")
    }

    #[inline]
    pub(crate) fn set_old_value(&mut self, index: usize, value: Value) {
        *self
            .old_values
            .get_mut(index)
            .expect("ICE: old payload value index out of bounds") = value;
    }

    fn reserve_nursery_span(&mut self, len: usize) -> PayloadRef {
        if len == 0 {
            return Self::empty();
        }
        let start = self.nursery_values.len();
        self.nursery_values.resize(start + len, Value::Unit);
        PayloadRef {
            start,
            end: start + len,
        }
    }

    fn take_old_free_span(&mut self, len: usize) -> Option<PayloadRef> {
        let index = self.old_free.iter().position(|span| span.len() >= len)?;
        let span = *self.old_free.get(index)?;
        if span.len() == len {
            self.old_free.swap_remove(index);
            Some(span)
        } else {
            let result = PayloadRef {
                start: span.start,
                end: span.start + len,
            };
            *self.old_free.get_mut(index)? = PayloadRef {
                start: span.start + len,
                end: span.end,
            };
            Some(result)
        }
    }

    fn reserve_old_span(&mut self, len: usize) -> PayloadRef {
        if len == 0 {
            return Self::empty();
        }

        let start = match len {
            1 => self.old_bucket1.pop(),
            2 => self.old_bucket2.pop(),
            3 => self.old_bucket3.pop(),
            _ => None,
        };
        if let Some(start) = start {
            return PayloadRef {
                start,
                end: start + len,
            };
        }
        if let Some(span) = self.take_old_free_span(len) {
            return span;
        }

        let start = self.old_values.len();
        self.old_values.resize(start + len, Value::Unit);
        PayloadRef {
            start,
            end: start + len,
        }
    }

    pub(crate) fn alloc_nursery_from_stack_tail(
        &mut self,
        stack: &mut Vec<Value>,
        len: usize,
    ) -> PayloadRef {
        if len == 0 {
            return Self::empty();
        }
        let stack_start = stack.len() - len;
        let span = self.reserve_nursery_span(len);
        let source = stack
            .get(stack_start..)
            .expect("ICE: stack payload source out of bounds");
        self.nursery_values
            .get_mut(span.start..span.end)
            .expect("ICE: nursery payload destination out of bounds")
            .copy_from_slice(source);
        stack.truncate(stack_start);
        span
    }

    pub(crate) fn alloc_nursery_clone(
        &mut self,
        source_is_nursery: bool,
        source: PayloadRef,
    ) -> PayloadRef {
        if source.is_empty() {
            return Self::empty();
        }
        let values = self.as_slice(source_is_nursery, source).to_vec();
        let destination = self.reserve_nursery_span(values.len());
        self.nursery_values
            .get_mut(destination.start..destination.end)
            .expect("ICE: nursery payload clone destination out of bounds")
            .copy_from_slice(&values);
        destination
    }

    pub(crate) fn alloc_nursery_concat(
        &mut self,
        source_is_nursery: Option<bool>,
        prefix: PayloadRef,
        stack: &mut Vec<Value>,
        suffix_len: usize,
    ) -> PayloadRef {
        let prefix_values = if prefix.is_empty() {
            Vec::new()
        } else {
            let Some(source_is_nursery) = source_is_nursery else {
                solveig_base::ice!("non-empty payload prefix has no owning object");
            };
            self.as_slice(source_is_nursery, prefix).to_vec()
        };
        let total = prefix_values.len() + suffix_len;
        if total == 0 {
            return Self::empty();
        }

        let stack_start = stack.len() - suffix_len;
        let suffix = stack
            .get(stack_start..)
            .expect("ICE: stack suffix source out of bounds")
            .to_vec();
        let destination = self.reserve_nursery_span(total);
        let values = self
            .nursery_values
            .get_mut(destination.start..destination.end)
            .expect("ICE: nursery payload concat destination out of bounds");
        let split = prefix_values.len();
        values
            .get_mut(..split)
            .expect("ICE: nursery payload prefix destination out of bounds")
            .copy_from_slice(&prefix_values);
        values
            .get_mut(split..)
            .expect("ICE: nursery payload suffix destination out of bounds")
            .copy_from_slice(&suffix);
        stack.truncate(stack_start);
        destination
    }

    pub(crate) fn copy_nursery_to_old(&mut self, source: PayloadRef) -> PayloadRef {
        if source.is_empty() {
            return Self::empty();
        }
        let values = self
            .nursery_values
            .get(source.start..source.end)
            .expect("ICE: promoted nursery payload span out of bounds")
            .to_vec();
        let destination = self.reserve_old_span(values.len());
        self.old_values
            .get_mut(destination.start..destination.end)
            .expect("ICE: promoted old payload span out of bounds")
            .copy_from_slice(&values);
        destination
    }

    pub(crate) fn free_old_span(&mut self, span: PayloadRef) {
        match span.len() {
            0 => {}
            1 => self.old_bucket1.push(span.start),
            2 => self.old_bucket2.push(span.start),
            3 => self.old_bucket3.push(span.start),
            _ => self.old_free.push(span),
        }
    }

    pub(crate) fn clear_nursery(&mut self) {
        self.nursery_values.clear();
    }

    fn alloc_nursery_from_values(&mut self, values: &[Value]) -> PayloadRef {
        let span = self.reserve_nursery_span(values.len());
        self.nursery_values
            .get_mut(span.start..span.end)
            .expect("ICE: nursery payload destination out of bounds")
            .copy_from_slice(values);
        span
    }

    fn alloc_old_from_values(&mut self, values: &[Value]) -> PayloadRef {
        let span = self.reserve_old_span(values.len());
        self.old_values
            .get_mut(span.start..span.end)
            .expect("ICE: old payload destination out of bounds")
            .copy_from_slice(values);
        span
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRef {
    pub start: usize,
    pub end: usize,
}

impl ByteRef {
    #[inline(always)]
    pub fn len(self) -> usize {
        self.end - self.start
    }

    #[inline(always)]
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

#[derive(Debug, Default)]
pub struct ByteStore {
    nursery_bytes: Vec<u8>,
    old_bytes: Vec<u8>,
    old_free: Vec<ByteRef>,
}

impl ByteStore {
    #[inline]
    fn empty() -> ByteRef {
        ByteRef { start: 0, end: 0 }
    }

    #[inline]
    fn as_slice(&self, nursery: bool, span: ByteRef) -> &[u8] {
        let bytes = if nursery {
            &self.nursery_bytes
        } else {
            &self.old_bytes
        };
        bytes
            .get(span.start..span.end)
            .expect("ICE: byte store span out of bounds")
    }

    #[inline]
    fn as_str(&self, nursery: bool, span: ByteRef) -> &str {
        std::str::from_utf8(self.as_slice(nursery, span))
            .expect("ICE: Solveig heap string bytes must be valid UTF-8")
    }

    fn reserve_nursery_span(&mut self, len: usize) -> ByteRef {
        if len == 0 {
            return Self::empty();
        }
        let start = self.nursery_bytes.len();
        self.nursery_bytes.resize(start + len, 0);
        ByteRef {
            start,
            end: start + len,
        }
    }

    fn take_old_free_span(&mut self, len: usize) -> Option<ByteRef> {
        let index = self.old_free.iter().position(|span| span.len() >= len)?;
        let span = *self.old_free.get(index)?;
        if span.len() == len {
            self.old_free.swap_remove(index);
            Some(span)
        } else {
            let result = ByteRef {
                start: span.start,
                end: span.start + len,
            };
            *self.old_free.get_mut(index)? = ByteRef {
                start: span.start + len,
                end: span.end,
            };
            Some(result)
        }
    }

    fn reserve_old_span(&mut self, len: usize) -> ByteRef {
        if len == 0 {
            return Self::empty();
        }
        if let Some(span) = self.take_old_free_span(len) {
            return span;
        }
        let start = self.old_bytes.len();
        self.old_bytes.resize(start + len, 0);
        ByteRef {
            start,
            end: start + len,
        }
    }

    fn alloc_nursery(&mut self, bytes: &[u8]) -> ByteRef {
        let span = self.reserve_nursery_span(bytes.len());
        self.nursery_bytes
            .get_mut(span.start..span.end)
            .expect("ICE: nursery byte allocation span out of bounds")
            .copy_from_slice(bytes);
        span
    }

    fn copy_nursery_to_old(&mut self, source: ByteRef) -> ByteRef {
        if source.is_empty() {
            return Self::empty();
        }
        let bytes = self
            .nursery_bytes
            .get(source.start..source.end)
            .expect("ICE: promoted nursery byte span out of bounds")
            .to_vec();
        let destination = self.reserve_old_span(bytes.len());
        self.old_bytes
            .get_mut(destination.start..destination.end)
            .expect("ICE: promoted old byte span out of bounds")
            .copy_from_slice(&bytes);
        destination
    }

    fn free_old_span(&mut self, span: ByteRef) {
        if !span.is_empty() {
            self.old_free.push(span);
        }
    }

    fn clear_nursery(&mut self) {
        self.nursery_bytes.clear();
    }
}
