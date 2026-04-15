#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Unit,
    Int(i64),
    CtorFn { tag: u16, arity: u16 },
    FunctionRef { chunk: u32, arity: u16 },
    Obj(ObjId),
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
}

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
        self.tags[tag as usize].expect("missing nullary constructor")
    }

    pub fn set(&mut self, tag: u16, val: Value) {
        self.tags[tag as usize] = Some(val);
    }
}
