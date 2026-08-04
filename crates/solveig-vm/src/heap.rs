//! Managed heap storage for VM values, closures, thunks, and constructors.
//! Garbage collection and runtime allocation share this ownership boundary.

use crate::values::{ObjId, Value};

const DEFAULT_GC_INITIAL_THRESHOLD: usize = 32_768;
const DEFAULT_GC_LIVE_FLOOR: usize = 65_536;
const DEFAULT_GC_STRESS_THRESHOLD: usize = 256;

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
    IoAction(IoAction),
}

#[derive(Debug, Clone, Copy)]
pub enum IoAction {
    Pure(Value),
    Bind {
        action: Value,
        continuation: Value,
    },
    Print {
        text: Value,
        newline: bool,
    },
    ReadLine,
    GetArgs,
    ReadFile {
        path: Value,
    },
    WriteFile {
        path: Value,
        contents: Value,
        append: bool,
    },
}

#[derive(Debug)]
pub struct Heap {
    pub objects: Vec<Option<HeapObject>>,
    pub mark_bits: Vec<bool>,
    pub free_list: Vec<u32>,
    pub payloads: PayloadStore,
    pub bytes: ByteStore,
    pub allocations_since_gc: usize,
    pub next_gc_at: usize,
    pub gc_stress: bool,
    pub gc_threshold_override: Option<usize>,
}

impl Default for Heap {
    fn default() -> Self {
        let gc_stress = gc_stress_enabled();
        let gc_threshold_override = gc_threshold_override();
        let next_gc_at = initial_gc_threshold(gc_stress, gc_threshold_override);

        Self {
            objects: Vec::new(),
            mark_bits: Vec::new(),
            free_list: Vec::new(),
            payloads: PayloadStore::default(),
            bytes: ByteStore::default(),
            allocations_since_gc: 0,
            next_gc_at,
            gc_stress,
            gc_threshold_override,
        }
    }
}

impl Heap {
    #[inline]
    pub fn should_collect(&self) -> bool {
        self.allocations_since_gc >= self.next_gc_at
    }

    pub fn reset_collection_accounting(&mut self, live: usize) {
        self.allocations_since_gc = 0;
        self.next_gc_at =
            next_gc_threshold_after_collection(self.gc_stress, self.gc_threshold_override, live);
    }

    #[inline]
    pub fn get(&self, id: ObjId) -> &HeapObject {
        let idx = id.0 as usize;
        debug_assert!(
            self.objects.get(idx).is_some_and(Option::is_some),
            "dangling heap ref {}",
            id.0
        );
        // SAFETY: Value::Obj is only constructed from ids returned by Heap::alloc,
        // and the debug assertion above checks that the slot is still live.
        let slot = unsafe { self.objects.get_unchecked(idx) };
        // SAFETY: The same invariant proves the live slot contains an object.
        unsafe { slot.as_ref().unwrap_unchecked() }
    }

    #[inline]
    pub fn alloc(&mut self, obj: HeapObject) -> Value {
        self.allocations_since_gc += 1;

        if let Some(id) = self.free_list.pop() {
            let idx = id as usize;
            *self
                .objects
                .get_mut(idx)
                .expect("ICE: heap free list object index out of bounds") = Some(obj);
            *self
                .mark_bits
                .get_mut(idx)
                .expect("ICE: heap free list mark-bit index out of bounds") = false;
            Value::Obj(ObjId(id))
        } else {
            let id = ObjId(
                u32::try_from(self.objects.len())
                    .unwrap_or_else(|_| solveig_base::ice!("heap object id overflow")),
            );
            self.objects.push(Some(obj));
            self.mark_bits.push(false);
            Value::Obj(id)
        }
    }

    #[inline(always)]
    pub fn alloc_data2(&mut self, tag: u16, a: Value, b: Value) -> Value {
        self.alloc(HeapObject::Data2 {
            tag,
            field0: a,
            field1: b,
        })
    }

    pub fn clone_payload(&mut self, span: PayloadRef) -> PayloadRef {
        self.payloads.alloc_clone(span)
    }

    #[inline]
    pub fn payload_len(&self, span: &PayloadRef) -> usize {
        PayloadStore::len(span)
    }

    #[inline]
    pub fn copy_payload_to_stack(&self, span: PayloadRef, stack: &mut [Value], dst_start: usize) {
        let src = self.payloads.as_slice(span);
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
        self.payloads.alloc_from_stack_tail(stack, n)
    }

    pub fn alloc_concat_payload_and_stack_tail(
        &mut self,
        prefix: PayloadRef,
        stack: &mut Vec<Value>,
        suffix_len: usize,
    ) -> PayloadRef {
        self.payloads
            .alloc_concat_from_payload_and_stack_tail(prefix, stack, suffix_len)
    }

    pub fn free_object_payloads(&mut self, obj: &HeapObject) {
        match obj {
            HeapObject::Closure { captures, .. } => self.payloads.free_span(*captures),
            HeapObject::PartialApp {
                captures,
                applied_args,
                ..
            } => {
                self.payloads.free_span(*captures);
                self.payloads.free_span(*applied_args);
            }
            HeapObject::PartialCtor { applied_args, .. } => self.payloads.free_span(*applied_args),
            HeapObject::DataN { fields, .. } => self.payloads.free_span(*fields),
            HeapObject::Str { bytes } => self.bytes.free_span(*bytes),
            HeapObject::Data0 { .. }
            | HeapObject::Data1 { .. }
            | HeapObject::Data2 { .. }
            | HeapObject::IoAction(_) => {}
        }
    }

    pub fn alloc_str(&mut self, value: &str) -> Value {
        let bytes = self.bytes.alloc(value.as_bytes());
        self.alloc(HeapObject::Str { bytes })
    }

    pub fn str_value(&self, bytes: ByteRef) -> &str {
        self.bytes.as_str(bytes)
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

fn initial_gc_threshold(gc_stress: bool, gc_threshold_override: Option<usize>) -> usize {
    gc_threshold_override.unwrap_or(if gc_stress {
        DEFAULT_GC_STRESS_THRESHOLD
    } else {
        DEFAULT_GC_INITIAL_THRESHOLD
    })
}

fn next_gc_threshold_after_collection(
    gc_stress: bool,
    gc_threshold_override: Option<usize>,
    live: usize,
) -> usize {
    gc_threshold_override.unwrap_or(if gc_stress {
        DEFAULT_GC_STRESS_THRESHOLD
    } else {
        live.max(DEFAULT_GC_LIVE_FLOOR) * 4
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gc_stress_uses_low_fixed_threshold_by_default() {
        assert_eq!(
            initial_gc_threshold(true, None),
            DEFAULT_GC_STRESS_THRESHOLD
        );
        assert_eq!(
            next_gc_threshold_after_collection(true, None, 100_000),
            DEFAULT_GC_STRESS_THRESHOLD
        );
    }

    #[test]
    fn explicit_gc_threshold_overrides_stress_and_adaptive_thresholds() {
        assert_eq!(initial_gc_threshold(true, Some(1)), 1);
        assert_eq!(initial_gc_threshold(false, Some(512)), 512);
        assert_eq!(next_gc_threshold_after_collection(true, Some(1), 100), 1);
        assert_eq!(
            next_gc_threshold_after_collection(false, Some(512), 100_000),
            512
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
    pub captures: PayloadRef,
    pub applied_args: PayloadRef,
}

impl BoundPrefix {
    pub const EMPTY: Self = Self {
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
    pub values: Vec<Value>,
    pub free: Vec<PayloadRef>,
    pub bucket1: Vec<usize>,
    pub bucket2: Vec<usize>,
    pub bucket3: Vec<usize>,
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
    bytes: Vec<u8>,
    free: Vec<ByteRef>,
}

impl ByteStore {
    #[inline]
    pub fn empty() -> ByteRef {
        ByteRef { start: 0, end: 0 }
    }

    #[inline]
    fn as_slice(&self, span: ByteRef) -> &[u8] {
        self.bytes
            .get(span.start..span.end)
            .expect("ICE: byte store span out of bounds")
    }

    #[inline]
    pub fn as_str(&self, span: ByteRef) -> &str {
        std::str::from_utf8(self.as_slice(span))
            .expect("ICE: Solveig heap string bytes must be valid UTF-8")
    }

    fn take_free_span(&mut self, len: usize) -> Option<ByteRef> {
        let idx = self.free.iter().position(|r| r.len() >= len)?;
        let span = *self.free.get(idx)?;

        if span.len() == len {
            self.free.swap_remove(idx);
            Some(span)
        } else {
            let out = ByteRef {
                start: span.start,
                end: span.start + len,
            };
            *self.free.get_mut(idx)? = ByteRef {
                start: span.start + len,
                end: span.end,
            };
            Some(out)
        }
    }

    fn reserve_span(&mut self, len: usize) -> ByteRef {
        if len == 0 {
            return Self::empty();
        }

        if let Some(span) = self.take_free_span(len) {
            return span;
        }

        let start = self.bytes.len();
        self.bytes.resize(start + len, 0);
        ByteRef {
            start,
            end: start + len,
        }
    }

    pub fn alloc(&mut self, bytes: &[u8]) -> ByteRef {
        let span = self.reserve_span(bytes.len());

        if !span.is_empty() {
            self.bytes
                .get_mut(span.start..span.end)
                .expect("ICE: byte store allocation span out of bounds")
                .copy_from_slice(bytes);
        }

        span
    }

    pub fn free_span(&mut self, span: ByteRef) {
        if !span.is_empty() {
            self.free.push(span);
        }
    }
}

impl PayloadStore {
    #[inline]
    pub fn empty() -> PayloadRef {
        PayloadRef { start: 0, end: 0 }
    }

    #[inline]
    pub fn len(span: &PayloadRef) -> usize {
        span.end - span.start
    }

    #[inline]
    pub fn as_slice(&self, span: PayloadRef) -> &[Value] {
        self.values
            .get(span.start..span.end)
            .expect("ICE: payload store span out of bounds")
    }

    pub fn take_free_span(&mut self, len: usize) -> Option<PayloadRef> {
        let idx = self.free.iter().position(|r| Self::len(r) >= len)?;
        let span = *self.free.get(idx)?;
        if Self::len(&span) == len {
            self.free.swap_remove(idx);
            Some(span)
        } else {
            let out = PayloadRef {
                start: span.start,
                end: span.start + len,
            };
            *self.free.get_mut(idx)? = PayloadRef {
                start: span.start + len,
                end: span.end,
            };
            Some(out)
        }
    }

    pub fn reserve_span(&mut self, len: usize) -> PayloadRef {
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
            return PayloadRef {
                start,
                end: start + len,
            };
        }

        if let Some(span) = self.take_free_span(len) {
            return span;
        }

        let start = self.values.len();
        self.values.resize(start + len, Value::Unit);
        PayloadRef {
            start,
            end: start + len,
        }
    }

    pub fn alloc_from_stack_tail(&mut self, stack: &mut Vec<Value>, n: usize) -> PayloadRef {
        if n == 0 {
            return Self::empty();
        }

        let stack_start = stack.len() - n;
        let span = self.reserve_span(n);
        let src = stack
            .get(stack_start..)
            .expect("ICE: stack payload source out of bounds");
        self.values
            .get_mut(span.start..span.end)
            .expect("ICE: payload store allocation span out of bounds")
            .copy_from_slice(src);
        stack.truncate(stack_start);
        span
    }

    pub fn alloc_clone(&mut self, src: PayloadRef) -> PayloadRef {
        let len = Self::len(&src);
        if len == 0 {
            return Self::empty();
        }

        let dst = self.reserve_span(len);
        self.values.copy_within(src.start..src.end, dst.start);
        dst
    }

    pub fn alloc_concat_from_payload_and_stack_tail(
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
            self.values.copy_within(prefix.start..prefix.end, dst.start);
        }

        if suffix_len > 0 {
            let stack_start = stack.len() - suffix_len;
            let dst_start = dst.start + prefix_len;
            let dst_end = dst_start + suffix_len;
            let src = stack
                .get(stack_start..)
                .expect("ICE: stack suffix source out of bounds");
            self.values
                .get_mut(dst_start..dst_end)
                .expect("ICE: payload concat destination out of bounds")
                .copy_from_slice(src);
            stack.truncate(stack_start);
        }

        dst
    }

    pub fn free_span(&mut self, span: PayloadRef) {
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
