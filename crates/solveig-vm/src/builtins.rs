use std::marker::PhantomData;

use solveig_ast::ast::{BUILTIN_PRIM_ADD_INT, BuiltinId};

use crate::heap::Heap;
use crate::values::Value;
use crate::vm::RuntimeError;

pub type BuiltinRun = fn(&mut VmCtx<'_>, &[Value]) -> Result<Value, RuntimeError>;

pub struct Builtin {
    pub id: BuiltinId,
    pub name: &'static str,
    pub arity: usize,
    pub run: BuiltinRun,
}

pub struct VmCtx<'a> {
    _marker: PhantomData<&'a mut Heap>,
}

impl<'a> VmCtx<'a> {
    pub(crate) fn new(_heap: &'a mut Heap) -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

pub fn lookup(id: BuiltinId) -> Option<&'static Builtin> {
    ALL_BUILTINS.iter().find(|builtin| builtin.id == id)
}

static ALL_BUILTINS: &[Builtin] = &[Builtin {
    id: BUILTIN_PRIM_ADD_INT,
    name: "prelude.prim.primAddInt",
    arity: 2,
    run: prim_add_int,
}];

fn prim_add_int(_ctx: &mut VmCtx<'_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let [Value::Int(lhs), Value::Int(rhs)] = args else {
        return Err(RuntimeError::builtin(
            "primAddInt expected two Int arguments".to_string(),
        ));
    };

    Ok(Value::Int(lhs + rhs))
}
