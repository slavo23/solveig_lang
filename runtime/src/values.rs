#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Unit,
    Int(i64),
    CtorFn { tag: u16, arity: u16 },
    Obj(ObjId),
}

#[derive(Clone)]
pub enum GlobalSlot {
    Evaluated(Value),
    Thunk { chunk: usize },
    BlackHole,
}

#[derive(Clone, Copy)]
pub enum ReturnTo {
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

pub struct BoolCache {
    pub true_val: Value,
    pub false_val: Value,
}

pub struct NullaryCache {
    pub tags: Vec<Option<Value>>,
}

impl NullaryCache {
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
