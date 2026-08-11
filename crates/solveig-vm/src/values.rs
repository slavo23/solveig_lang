//! Runtime value and stack-frame representations.
//! Bytecode dispatch reads and writes these structures during execution.

use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Copy)]
pub struct ObjId(pub u32);

#[derive(Debug, Clone, Copy)]
pub enum Value {
    Unit,
    Int(i64),
    Float(f64),
    Char(char),
    CtorFn { tag: u16, arity: u16 },
    FunctionRef { chunk: u32, arity: u16 },
    Obj(ObjId),
}

impl PartialEq for ObjId {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for ObjId {}

impl Hash for ObjId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Unit, Value::Unit) => true,
            (Value::Int(lhs), Value::Int(rhs)) => lhs == rhs,
            (Value::Float(lhs), Value::Float(rhs)) => lhs.to_bits() == rhs.to_bits(),
            (Value::Char(lhs), Value::Char(rhs)) => lhs == rhs,
            (
                Value::CtorFn {
                    tag: lhs_tag,
                    arity: lhs_arity,
                },
                Value::CtorFn {
                    tag: rhs_tag,
                    arity: rhs_arity,
                },
            ) => lhs_tag == rhs_tag && lhs_arity == rhs_arity,
            (
                Value::FunctionRef {
                    chunk: lhs_chunk,
                    arity: lhs_arity,
                },
                Value::FunctionRef {
                    chunk: rhs_chunk,
                    arity: rhs_arity,
                },
            ) => lhs_chunk == rhs_chunk && lhs_arity == rhs_arity,
            (Value::Obj(lhs), Value::Obj(rhs)) => lhs == rhs,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Value::Unit => {}
            Value::Int(value) => value.hash(state),
            Value::Float(value) => value.to_bits().hash(state),
            Value::Char(value) => value.hash(state),
            Value::CtorFn { tag, arity } => {
                tag.hash(state);
                arity.hash(state);
            }
            Value::FunctionRef { chunk, arity } => {
                chunk.hash(state);
                arity.hash(state);
            }
            Value::Obj(id) => id.hash(state),
        }
    }
}

#[derive(Clone)]
pub enum GlobalSlot {
    Evaluated(Value),
    Thunk { chunk: usize },
    Evaluating,
}

#[derive(Clone, Copy)]
pub enum Continuation {
    Halt,
    CallerFrame {
        chunk: usize,
        pc: usize,
        frame_base: usize,
    },
    PublishGlobalThenHalt {
        global_index: usize,
    },
    PublishGlobalThenResume {
        global_index: usize,
        chunk: usize,
        pc: usize,
        frame_base: usize,
    },
    /// Applies this function to the value produced by the IO action below it.
    IoBind(Value),
}

// The pre-I1 layout measured 40 bytes; IO frames must not widen this hot type.
const _: () = assert!(std::mem::size_of::<Continuation>() <= 40);

pub struct BoolCache {
    pub true_val: Value,
    pub false_val: Value,
}

pub struct NullaryCtorCache {
    pub tags: Vec<Option<Value>>,
}

impl NullaryCtorCache {
    pub fn new(max_tag: usize) -> Self {
        Self {
            tags: vec![None; max_tag + 1],
        }
    }

    #[inline]
    pub fn get(&self, tag: u16) -> Value {
        self.tags
            .get(tag as usize)
            .and_then(|slot| *slot)
            .expect("ICE: missing nullary constructor")
    }

    #[inline]
    pub fn try_get(&self, tag: u16) -> Option<Value> {
        self.tags.get(tag as usize).and_then(|slot| *slot)
    }

    pub fn set(&mut self, tag: u16, val: Value) {
        *self
            .tags
            .get_mut(tag as usize)
            .expect("ICE: nullary constructor tag out of bounds") = Some(val);
    }
}
