use crate::values::{ObjId, Value};

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
}

#[derive(Debug)]
pub struct Heap {
    pub objects: Vec<Option<HeapObject>>,
    pub mark_bits: Vec<bool>,
    pub free_list: Vec<u32>,
    pub payloads: PayloadStore,
    pub allocations_since_gc: usize,
    pub next_gc_at: usize,
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

impl Heap {
    #[inline]
    pub fn get(&self, id: ObjId) -> &HeapObject {
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
    pub fn alloc(&mut self, obj: HeapObject) -> Value {
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
        stack[dst_start..dst_start + src.len()].copy_from_slice(src);
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
            HeapObject::Data0 { .. } | HeapObject::Data1 { .. } | HeapObject::Data2 { .. } => {}
        }
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
        &self.values[span.start..span.end]
    }

    pub fn take_free_span(&mut self, len: usize) -> Option<PayloadRef> {
        let idx = self.free.iter().position(|r| Self::len(r) >= len)?;
        let span = self.free[idx];
        if Self::len(&span) == len {
            self.free.swap_remove(idx);
            Some(span)
        } else {
            let out = PayloadRef {
                start: span.start,
                end: span.start + len,
            };
            self.free[idx] = PayloadRef {
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
        self.values.reserve(len);
        unsafe {
            self.values.set_len(start + len);
        }
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
        self.values[span.start..span.end].copy_from_slice(&stack[stack_start..]);
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
            self.values[dst_start..dst_end].copy_from_slice(&stack[stack_start..]);
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
