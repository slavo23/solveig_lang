//! Runtime implementations of Solveig primitive operations.
//! The VM dispatches builtin ids here for arithmetic, strings, files, and IO.

use std::cmp::Ordering;

use solveig_ast::ast::{
    BUILTIN_APPEND_FILE, BUILTIN_BIND_IO, BUILTIN_CHAR_COMPARE, BUILTIN_CHAR_EQ,
    BUILTIN_CHAR_TO_STRING, BUILTIN_CHARS_TO_STRING, BUILTIN_FLOAT_ADD, BUILTIN_FLOAT_DIV,
    BUILTIN_FLOAT_EQ, BUILTIN_FLOAT_GE, BUILTIN_FLOAT_GT, BUILTIN_FLOAT_LE, BUILTIN_FLOAT_LT,
    BUILTIN_FLOAT_MUL, BUILTIN_FLOAT_NEG, BUILTIN_FLOAT_SUB, BUILTIN_FLOAT_TO_INT,
    BUILTIN_FLOAT_TO_STRING, BUILTIN_GET_ARGS, BUILTIN_INT_ADD, BUILTIN_INT_COMPARE,
    BUILTIN_INT_DIV, BUILTIN_INT_EQ, BUILTIN_INT_GE, BUILTIN_INT_GT, BUILTIN_INT_LE,
    BUILTIN_INT_LT, BUILTIN_INT_MOD, BUILTIN_INT_MUL, BUILTIN_INT_NEG, BUILTIN_INT_SUB,
    BUILTIN_INT_TO_FLOAT, BUILTIN_INT_TO_STRING, BUILTIN_PRINT_LINE, BUILTIN_PRINT_STR,
    BUILTIN_PURE_IO, BUILTIN_READ_FILE, BUILTIN_READ_LINE, BUILTIN_STRING_APPEND,
    BUILTIN_STRING_CHAR_AT, BUILTIN_STRING_COMPARE, BUILTIN_STRING_EQ, BUILTIN_STRING_LENGTH,
    BUILTIN_STRING_SLICE, BUILTIN_STRING_TO_CHARS, BUILTIN_WRITE_FILE, BuiltinId, CtorRuntimeInfo,
    CtorRuntimeRole,
};

use crate::heap::{HeapObject, IoAction};
use crate::values::Value;
use crate::vm::{Interpreter, RuntimeError};

pub type BuiltinRun = for<'context, 'runtime> fn(
    &mut VmCtx<'context, 'runtime>,
    &[Value],
) -> Result<Value, RuntimeError>;

pub struct Builtin {
    pub id: BuiltinId,
    pub name: &'static str,
    pub arity: usize,
    pub run: BuiltinRun,
}

pub struct VmCtx<'context, 'runtime> {
    interpreter: &'context mut Interpreter<'runtime>,
}

impl<'context, 'runtime> VmCtx<'context, 'runtime> {
    pub(crate) fn new(interpreter: &'context mut Interpreter<'runtime>) -> Self {
        Self { interpreter }
    }

    fn bool_value(&self, value: bool) -> Value {
        if value {
            self.interpreter.bools.true_val
        } else {
            self.interpreter.bools.false_val
        }
    }

    fn alloc_str(&mut self, value: &str) -> Value {
        self.interpreter.heap.alloc_str(value)
    }

    fn alloc_io_action(&mut self, action: IoAction) -> Value {
        self.interpreter.heap.alloc(HeapObject::IoAction(action))
    }

    fn ctor_info(
        &self,
        role: CtorRuntimeRole,
        builtin: &str,
    ) -> Result<&CtorRuntimeInfo, RuntimeError> {
        self.interpreter
            .program
            .constructor_arities
            .iter()
            .find(|info| info.role == role)
            .ok_or_else(|| {
                RuntimeError::builtin(format!(
                    "{builtin} runtime constructor metadata missing for role {role:?}"
                ))
            })
    }

    fn ctor_tag_with_arity(
        &self,
        role: CtorRuntimeRole,
        arity: u16,
        builtin: &str,
    ) -> Result<u16, RuntimeError> {
        let info = self.ctor_info(role, builtin)?;
        if info.arity != arity {
            return Err(RuntimeError::builtin(format!(
                "{builtin} runtime constructor `{}` expected arity {}, got {}",
                info.short_name, arity, info.arity
            )));
        }
        Ok(info.tag)
    }

    fn nullary_ctor(&self, role: CtorRuntimeRole, builtin: &str) -> Result<Value, RuntimeError> {
        let tag = self.ctor_tag_with_arity(role, 0, builtin)?;
        self.interpreter.nullary_ctors.try_get(tag).ok_or_else(|| {
            RuntimeError::builtin(format!(
                "{builtin} runtime nullary constructor cache missing tag {tag}"
            ))
        })
    }

    fn alloc_data1_ctor(
        &mut self,
        role: CtorRuntimeRole,
        field: Value,
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        let tag = self.ctor_tag_with_arity(role, 1, builtin)?;
        Ok(self
            .interpreter
            .heap
            .alloc(HeapObject::Data1 { tag, field }))
    }

    pub(crate) fn alloc_list_from_values(
        &mut self,
        values: &[Value],
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        let checkpoint = self.interpreter.process.temporary_roots.len();
        self.interpreter
            .process
            .temporary_roots
            .extend_from_slice(values);
        let result = self.alloc_list_from_root_range(checkpoint, values.len(), builtin);
        self.interpreter
            .process
            .temporary_roots
            .truncate(checkpoint);
        result
    }

    pub(crate) fn alloc_list_from_root_range(
        &mut self,
        root_start: usize,
        value_count: usize,
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        let cons_tag = self.ctor_tag_with_arity(CtorRuntimeRole::PreludeListCons, 2, builtin)?;
        let nil = self.nullary_ctor(CtorRuntimeRole::PreludeListNil, builtin)?;
        let result_root = self.interpreter.process.temporary_roots.len();
        self.interpreter.process.temporary_roots.push(nil);

        for offset in (0..value_count).rev() {
            self.interpreter.safe_point();
            let value = *self
                .interpreter
                .process
                .temporary_roots
                .get(root_start + offset)
                .expect("ICE: rooted list element index out of bounds");
            let result = *self
                .interpreter
                .process
                .temporary_roots
                .get(result_root)
                .expect("ICE: rooted list result index out of bounds");
            let result = self.interpreter.heap.alloc(HeapObject::Data2 {
                tag: cons_tag,
                field0: value,
                field1: result,
            });
            *self
                .interpreter
                .process
                .temporary_roots
                .get_mut(result_root)
                .expect("ICE: rooted list result index out of bounds") = result;
        }

        let result = self
            .interpreter
            .process
            .temporary_roots
            .pop()
            .expect("ICE: rooted list result disappeared");
        Ok(result)
    }
}

pub fn lookup(id: BuiltinId) -> Option<&'static Builtin> {
    ALL_BUILTINS.iter().find(|builtin| builtin.id == id)
}

macro_rules! builtin {
    ($id:expr, $name:literal, $arity:expr, $run:ident) => {
        Builtin {
            id: $id,
            name: concat!("prelude.prim.", $name),
            arity: $arity,
            run: $run,
        }
    };
}

static ALL_BUILTINS: &[Builtin] = &[
    builtin!(BUILTIN_INT_ADD, "intAdd", 2, int_add),
    builtin!(BUILTIN_INT_SUB, "intSub", 2, int_sub),
    builtin!(BUILTIN_INT_MUL, "intMul", 2, int_mul),
    builtin!(BUILTIN_INT_DIV, "intDiv", 2, int_div),
    builtin!(BUILTIN_INT_MOD, "intMod", 2, int_mod),
    builtin!(BUILTIN_INT_NEG, "intNeg", 1, int_neg),
    builtin!(BUILTIN_INT_EQ, "intEq", 2, int_eq),
    builtin!(BUILTIN_INT_LT, "intLt", 2, int_lt),
    builtin!(BUILTIN_INT_LE, "intLe", 2, int_le),
    builtin!(BUILTIN_INT_GT, "intGt", 2, int_gt),
    builtin!(BUILTIN_INT_GE, "intGe", 2, int_ge),
    builtin!(BUILTIN_INT_COMPARE, "intCompare", 2, int_compare),
    builtin!(BUILTIN_FLOAT_ADD, "floatAdd", 2, float_add),
    builtin!(BUILTIN_FLOAT_SUB, "floatSub", 2, float_sub),
    builtin!(BUILTIN_FLOAT_MUL, "floatMul", 2, float_mul),
    builtin!(BUILTIN_FLOAT_DIV, "floatDiv", 2, float_div),
    builtin!(BUILTIN_FLOAT_NEG, "floatNeg", 1, float_neg),
    builtin!(BUILTIN_FLOAT_EQ, "floatEq", 2, float_eq),
    builtin!(BUILTIN_FLOAT_LT, "floatLt", 2, float_lt),
    builtin!(BUILTIN_FLOAT_LE, "floatLe", 2, float_le),
    builtin!(BUILTIN_FLOAT_GT, "floatGt", 2, float_gt),
    builtin!(BUILTIN_FLOAT_GE, "floatGe", 2, float_ge),
    builtin!(BUILTIN_INT_TO_FLOAT, "intToFloat", 1, int_to_float),
    builtin!(BUILTIN_FLOAT_TO_INT, "floatToInt", 1, float_to_int),
    builtin!(BUILTIN_INT_TO_STRING, "intToString", 1, int_to_string),
    builtin!(BUILTIN_FLOAT_TO_STRING, "floatToString", 1, float_to_string),
    builtin!(BUILTIN_CHAR_TO_STRING, "charToString", 1, char_to_string),
    builtin!(BUILTIN_STRING_LENGTH, "stringLength", 1, string_length),
    builtin!(BUILTIN_STRING_APPEND, "stringAppend", 2, string_append),
    builtin!(BUILTIN_STRING_SLICE, "stringSlice", 3, string_slice),
    builtin!(BUILTIN_STRING_CHAR_AT, "stringCharAt", 2, string_char_at),
    builtin!(BUILTIN_STRING_EQ, "stringEq", 2, string_eq),
    builtin!(BUILTIN_STRING_COMPARE, "stringCompare", 2, string_compare),
    builtin!(BUILTIN_CHAR_EQ, "charEq", 2, char_eq),
    builtin!(BUILTIN_CHAR_COMPARE, "charCompare", 2, char_compare),
    builtin!(BUILTIN_STRING_TO_CHARS, "stringToChars", 1, string_to_chars),
    builtin!(BUILTIN_CHARS_TO_STRING, "charsToString", 1, chars_to_string),
    builtin!(BUILTIN_PURE_IO, "pureIO", 1, pure_io),
    builtin!(BUILTIN_BIND_IO, "bindIO", 2, bind_io),
    builtin!(BUILTIN_PRINT_LINE, "printLine", 1, print_line),
    builtin!(BUILTIN_PRINT_STR, "printStr", 1, print_str),
    builtin!(BUILTIN_READ_LINE, "readLine", 0, read_line),
    builtin!(BUILTIN_GET_ARGS, "getArgs", 0, get_args),
    builtin!(BUILTIN_READ_FILE, "readFile", 1, read_file),
    builtin!(BUILTIN_WRITE_FILE, "writeFile", 2, write_file),
    builtin!(BUILTIN_APPEND_FILE, "appendFile", 2, append_file),
];

// D3 primitive semantics:
// - Int add/sub/mul/neg wrap on i64 overflow. Division and modulo trap on zero;
//   i64::MIN divided/modded by -1 follows wrapping i64 behavior.
// - Float arithmetic and comparisons delegate to f64, preserving IEEE-754 NaN
//   and infinity behavior for arithmetic and relational predicates.
// - String length, indexing, slicing, and char bridges use Unicode scalar values.
//   Slicing clamps negative/out-of-range bounds instead of trapping.

pub(crate) fn int_div_trap(lhs: i64, rhs: i64, op_name: &str) -> Result<i64, RuntimeError> {
    if rhs == 0 {
        return Err(RuntimeError::builtin(format!("{op_name} division by zero")));
    }
    if lhs == i64::MIN && rhs == -1 {
        return Ok(i64::MIN);
    }
    Ok(lhs / rhs)
}

pub(crate) fn int_mod_trap(lhs: i64, rhs: i64, op_name: &str) -> Result<i64, RuntimeError> {
    if rhs == 0 {
        return Err(RuntimeError::builtin(format!("{op_name} modulo by zero")));
    }
    if lhs == i64::MIN && rhs == -1 {
        return Ok(0);
    }
    Ok(lhs % rhs)
}

fn int_add(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intAdd")?;
    Ok(Value::Int(lhs.wrapping_add(rhs)))
}

fn int_sub(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intSub")?;
    Ok(Value::Int(lhs.wrapping_sub(rhs)))
}

fn int_mul(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intMul")?;
    Ok(Value::Int(lhs.wrapping_mul(rhs)))
}

fn int_div(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intDiv")?;
    Ok(Value::Int(int_div_trap(lhs, rhs, "intDiv")?))
}

fn int_mod(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intMod")?;
    Ok(Value::Int(int_mod_trap(lhs, rhs, "intMod")?))
}

fn int_neg(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_int(args, 0, "intNeg")?;
    Ok(Value::Int(value.wrapping_neg()))
}

fn int_eq(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intEq")?;
    Ok(ctx.bool_value(lhs == rhs))
}

fn int_lt(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intLt")?;
    Ok(ctx.bool_value(lhs < rhs))
}

fn int_le(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intLe")?;
    Ok(ctx.bool_value(lhs <= rhs))
}

fn int_gt(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intGt")?;
    Ok(ctx.bool_value(lhs > rhs))
}

fn int_ge(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intGe")?;
    Ok(ctx.bool_value(lhs >= rhs))
}

fn int_compare(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intCompare")?;
    ordering_value(ctx, lhs.cmp(&rhs), "intCompare")
}

fn float_add(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatAdd")?;
    Ok(Value::Float(lhs + rhs))
}

fn float_sub(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatSub")?;
    Ok(Value::Float(lhs - rhs))
}

fn float_mul(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatMul")?;
    Ok(Value::Float(lhs * rhs))
}

fn float_div(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatDiv")?;
    Ok(Value::Float(lhs / rhs))
}

fn float_neg(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_float(args, 0, "floatNeg")?;
    Ok(Value::Float(-value))
}

fn float_eq(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatEq")?;
    Ok(ctx.bool_value(lhs == rhs))
}

fn float_lt(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatLt")?;
    Ok(ctx.bool_value(lhs < rhs))
}

fn float_le(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatLe")?;
    Ok(ctx.bool_value(lhs <= rhs))
}

fn float_gt(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatGt")?;
    Ok(ctx.bool_value(lhs > rhs))
}

fn float_ge(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatGe")?;
    Ok(ctx.bool_value(lhs >= rhs))
}

fn int_to_float(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_int(args, 0, "intToFloat")?;
    Ok(Value::Float(value as f64))
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "Solveig floatToInt is defined as Rust's saturating truncating f64-to-i64 conversion"
)]
fn float_to_int(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_float(args, 0, "floatToInt")?;
    // D3: truncate toward zero. Rust's f64 -> i64 cast is saturating and keeps
    // this conversion total for NaN and out-of-range values.
    Ok(Value::Int(value.trunc() as i64))
}

fn int_to_string(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_int(args, 0, "intToString")?;
    Ok(ctx.alloc_str(&value.to_string()))
}

fn float_to_string(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_float(args, 0, "floatToString")?;
    Ok(ctx.alloc_str(&value.to_string()))
}

fn char_to_string(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_char(args, 0, "charToString")?;
    Ok(ctx.alloc_str(&value.to_string()))
}

fn string_length(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_string_owned(ctx, args, 0, "stringLength")?;
    let len = i64::try_from(value.chars().count()).map_err(|_error| {
        RuntimeError::builtin("stringLength result does not fit in Int".to_string())
    })?;
    Ok(Value::Int(len))
}

fn string_append(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let lhs = expect_string_owned(ctx, args, 0, "stringAppend")?;
    let rhs = expect_string_owned(ctx, args, 1, "stringAppend")?;
    Ok(ctx.alloc_str(&(lhs + &rhs)))
}

fn string_slice(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_string_owned(ctx, args, 0, "stringSlice")?;
    let start = expect_int(args, 1, "stringSlice")?;
    let len = expect_int(args, 2, "stringSlice")?;
    let start = if start <= 0 {
        0
    } else {
        usize::try_from(start).unwrap_or(usize::MAX)
    };
    let len = if len <= 0 {
        0
    } else {
        usize::try_from(len).unwrap_or(usize::MAX)
    };
    let sliced = value.chars().skip(start).take(len).collect::<String>();
    Ok(ctx.alloc_str(&sliced))
}

fn string_char_at(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_string_owned(ctx, args, 0, "stringCharAt")?;
    let index = expect_int(args, 1, "stringCharAt")?;
    let Some(index) = usize::try_from(index).ok() else {
        return ctx.nullary_ctor(CtorRuntimeRole::PreludeMaybeNothing, "stringCharAt");
    };

    match value.chars().nth(index) {
        Some(ch) => ctx.alloc_data1_ctor(
            CtorRuntimeRole::PreludeMaybeJust,
            Value::Char(ch),
            "stringCharAt",
        ),
        None => ctx.nullary_ctor(CtorRuntimeRole::PreludeMaybeNothing, "stringCharAt"),
    }
}

fn string_eq(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let lhs = expect_string_owned(ctx, args, 0, "stringEq")?;
    let rhs = expect_string_owned(ctx, args, 1, "stringEq")?;
    Ok(ctx.bool_value(lhs == rhs))
}

fn string_compare(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let lhs = expect_string_owned(ctx, args, 0, "stringCompare")?;
    let rhs = expect_string_owned(ctx, args, 1, "stringCompare")?;
    ordering_value(ctx, lhs.cmp(&rhs), "stringCompare")
}

fn char_eq(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_char2(args, "charEq")?;
    Ok(ctx.bool_value(lhs == rhs))
}

fn char_compare(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_char2(args, "charCompare")?;
    ordering_value(ctx, lhs.cmp(&rhs), "charCompare")
}

fn string_to_chars(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_string_owned(ctx, args, 0, "stringToChars")?;
    let chars = value.chars().map(Value::Char).collect::<Vec<_>>();
    ctx.alloc_list_from_values(&chars, "stringToChars")
}

fn chars_to_string(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_value(args, 0, "charsToString")?;
    let string = expect_char_list(ctx, value, "charsToString")?;
    Ok(ctx.alloc_str(&string))
}

fn pure_io(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_value(args, 0, "pureIO")?;
    Ok(ctx.alloc_io_action(IoAction::Pure(value)))
}

fn bind_io(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let action = expect_value(args, 0, "bindIO")?;
    let continuation = expect_value(args, 1, "bindIO")?;
    Ok(ctx.alloc_io_action(IoAction::Bind {
        action,
        continuation,
    }))
}

fn print_line(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let text = expect_value(args, 0, "printLine")?;
    Ok(ctx.alloc_io_action(IoAction::Print {
        text,
        newline: true,
    }))
}

fn print_str(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let text = expect_value(args, 0, "printStr")?;
    Ok(ctx.alloc_io_action(IoAction::Print {
        text,
        newline: false,
    }))
}

fn read_line(ctx: &mut VmCtx<'_, '_>, _args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_io_action(IoAction::ReadLine))
}

fn get_args(ctx: &mut VmCtx<'_, '_>, _args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_io_action(IoAction::GetArgs))
}

fn read_file(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_value(args, 0, "readFile")?;
    Ok(ctx.alloc_io_action(IoAction::ReadFile { path }))
}

fn write_file(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_value(args, 0, "writeFile")?;
    let contents = expect_value(args, 1, "writeFile")?;
    Ok(ctx.alloc_io_action(IoAction::WriteFile {
        path,
        contents,
        append: false,
    }))
}

fn append_file(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_value(args, 0, "appendFile")?;
    let contents = expect_value(args, 1, "appendFile")?;
    Ok(ctx.alloc_io_action(IoAction::WriteFile {
        path,
        contents,
        append: true,
    }))
}

fn ordering_value(
    ctx: &VmCtx<'_, '_>,
    ordering: Ordering,
    builtin: &str,
) -> Result<Value, RuntimeError> {
    let ctor = match ordering {
        Ordering::Less => CtorRuntimeRole::PreludeOrderingLt,
        Ordering::Equal => CtorRuntimeRole::PreludeOrderingEq,
        Ordering::Greater => CtorRuntimeRole::PreludeOrderingGt,
    };
    ctx.nullary_ctor(ctor, builtin)
}

fn expect_value(args: &[Value], index: usize, builtin: &str) -> Result<Value, RuntimeError> {
    args.get(index).copied().ok_or_else(|| {
        RuntimeError::builtin(format!(
            "{builtin} expected argument at position {}",
            index + 1
        ))
    })
}

fn expect_int(args: &[Value], index: usize, builtin: &str) -> Result<i64, RuntimeError> {
    match expect_value(args, index, builtin)? {
        Value::Int(value) => Ok(value),
        _ => Err(RuntimeError::builtin(format!(
            "{builtin} expected Int argument at position {}",
            index + 1
        ))),
    }
}

fn expect_int2(args: &[Value], builtin: &str) -> Result<(i64, i64), RuntimeError> {
    Ok((expect_int(args, 0, builtin)?, expect_int(args, 1, builtin)?))
}

fn expect_float(args: &[Value], index: usize, builtin: &str) -> Result<f64, RuntimeError> {
    match expect_value(args, index, builtin)? {
        Value::Float(value) => Ok(value),
        _ => Err(RuntimeError::builtin(format!(
            "{builtin} expected Float argument at position {}",
            index + 1
        ))),
    }
}

fn expect_float2(args: &[Value], builtin: &str) -> Result<(f64, f64), RuntimeError> {
    Ok((
        expect_float(args, 0, builtin)?,
        expect_float(args, 1, builtin)?,
    ))
}

fn expect_char(args: &[Value], index: usize, builtin: &str) -> Result<char, RuntimeError> {
    match expect_value(args, index, builtin)? {
        Value::Char(value) => Ok(value),
        _ => Err(RuntimeError::builtin(format!(
            "{builtin} expected Char argument at position {}",
            index + 1
        ))),
    }
}

fn expect_char2(args: &[Value], builtin: &str) -> Result<(char, char), RuntimeError> {
    Ok((
        expect_char(args, 0, builtin)?,
        expect_char(args, 1, builtin)?,
    ))
}

fn expect_string_owned(
    ctx: &VmCtx<'_, '_>,
    args: &[Value],
    index: usize,
    builtin: &str,
) -> Result<String, RuntimeError> {
    match expect_value(args, index, builtin)? {
        Value::Obj(id) => match ctx.interpreter.heap.get(id) {
            HeapObject::Str { bytes } => Ok(ctx.interpreter.heap.str_value(id, *bytes).to_string()),
            _ => Err(RuntimeError::builtin(format!(
                "{builtin} expected String argument at position {}",
                index + 1
            ))),
        },
        _ => Err(RuntimeError::builtin(format!(
            "{builtin} expected String argument at position {}",
            index + 1
        ))),
    }
}

fn expect_char_list(
    ctx: &VmCtx<'_, '_>,
    mut current: Value,
    builtin: &str,
) -> Result<String, RuntimeError> {
    let nil_tag = ctx.ctor_tag_with_arity(CtorRuntimeRole::PreludeListNil, 0, builtin)?;
    let cons_tag = ctx.ctor_tag_with_arity(CtorRuntimeRole::PreludeListCons, 2, builtin)?;
    let mut out = String::new();

    loop {
        let Value::Obj(id) = current else {
            return Err(RuntimeError::builtin(format!(
                "{builtin} expected [Char] list spine"
            )));
        };

        match ctx.interpreter.heap.get(id) {
            HeapObject::Data0 { tag } if *tag == nil_tag => return Ok(out),
            HeapObject::Data2 {
                tag,
                field0,
                field1,
            } if *tag == cons_tag => {
                let Value::Char(ch) = field0 else {
                    return Err(RuntimeError::builtin(format!(
                        "{builtin} expected Char list element"
                    )));
                };
                out.push(*ch);
                current = *field1;
            }
            _ => {
                return Err(RuntimeError::builtin(format!(
                    "{builtin} expected [Char] list spine"
                )));
            }
        }
    }
}
