//! Text, character, and immutable byte-string primitives.
//! This module owns conversion semantics and GC-safe construction of results.

use solveig_ast::ast::CtorRuntimeRole;

use super::{
    VmCtx, expect_char, expect_char2, expect_float, expect_int, expect_value, ordering_value,
};
use crate::heap::HeapObject;
use crate::values::Value;
use crate::vm::RuntimeError;

pub(super) fn int_to_string(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_int(args, 0, "intToString")?;
    Ok(ctx.alloc_str(&value.to_string()))
}

pub(super) fn float_to_string(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_float(args, 0, "floatToString")?;
    Ok(ctx.alloc_str(&value.to_string()))
}

pub(super) fn char_to_string(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_char(args, 0, "charToString")?;
    Ok(ctx.alloc_str(&value.to_string()))
}

pub(super) fn string_length(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_string_owned(ctx, args, 0, "stringLength")?;
    let len = i64::try_from(value.chars().count()).map_err(|_error| {
        RuntimeError::builtin("stringLength result does not fit in Int".to_string())
    })?;
    Ok(Value::Int(len))
}

pub(super) fn string_append(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let lhs = expect_string_owned(ctx, args, 0, "stringAppend")?;
    let rhs = expect_string_owned(ctx, args, 1, "stringAppend")?;
    Ok(ctx.alloc_str(&(lhs + &rhs)))
}

pub(super) fn string_slice(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
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

pub(super) fn string_char_at(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
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

pub(super) fn string_eq(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let lhs = expect_string_owned(ctx, args, 0, "stringEq")?;
    let rhs = expect_string_owned(ctx, args, 1, "stringEq")?;
    Ok(ctx.bool_value(lhs == rhs))
}

pub(super) fn string_compare(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let lhs = expect_string_owned(ctx, args, 0, "stringCompare")?;
    let rhs = expect_string_owned(ctx, args, 1, "stringCompare")?;
    ordering_value(ctx, lhs.cmp(&rhs), "stringCompare")
}

pub(super) fn char_eq(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_char2(args, "charEq")?;
    Ok(ctx.bool_value(lhs == rhs))
}

pub(super) fn char_compare(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_char2(args, "charCompare")?;
    ordering_value(ctx, lhs.cmp(&rhs), "charCompare")
}

pub(super) fn string_to_chars(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_string_owned(ctx, args, 0, "stringToChars")?;
    let chars = value.chars().map(Value::Char).collect::<Vec<_>>();
    ctx.alloc_list_from_values(&chars, "stringToChars")
}

pub(super) fn chars_to_string(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_value(args, 0, "charsToString")?;
    let string = expect_char_list(ctx, value, "charsToString")?;
    Ok(ctx.alloc_str(&string))
}

pub(super) fn string_to_bytes(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_string_owned(ctx, args, 0, "stringToBytes")?;
    Ok(ctx.alloc_bytes(value.as_bytes()))
}

pub(super) fn bytes_to_string(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_bytes_owned(ctx, args, 0, "bytesToString")?;
    match String::from_utf8(value) {
        Ok(text) => alloc_valid_utf8_result(ctx, &text),
        Err(error) => alloc_invalid_utf8_result(ctx, &error.to_string()),
    }
}

fn alloc_valid_utf8_result(ctx: &mut VmCtx<'_, '_>, text: &str) -> Result<Value, RuntimeError> {
    let tag = ctx.ctor_tag_with_arity(CtorRuntimeRole::PreludeResultOk, 1, "bytesToString")?;
    let checkpoint = ctx.interpreter.process.temporary_roots.len();
    let text = ctx.alloc_str(text);
    ctx.interpreter.process.temporary_roots.push(text);
    ctx.interpreter.safe_point();
    let text = temporary_root(ctx, checkpoint, "text");
    let result = ctx
        .interpreter
        .heap
        .alloc(HeapObject::Data1 { tag, field: text });
    ctx.interpreter.process.temporary_roots.truncate(checkpoint);
    Ok(result)
}

fn alloc_invalid_utf8_result(
    ctx: &mut VmCtx<'_, '_>,
    message: &str,
) -> Result<Value, RuntimeError> {
    let invalid_tag = ctx.ctor_tag_with_arity(
        CtorRuntimeRole::PreludeIoErrorInvalidUtf8,
        1,
        "bytesToString",
    )?;
    let result_tag =
        ctx.ctor_tag_with_arity(CtorRuntimeRole::PreludeResultErr, 1, "bytesToString")?;
    let checkpoint = ctx.interpreter.process.temporary_roots.len();
    let message = ctx.alloc_str(message);
    ctx.interpreter.process.temporary_roots.push(message);
    ctx.interpreter.safe_point();
    let message = temporary_root(ctx, checkpoint, "error");
    let invalid = ctx.interpreter.heap.alloc(HeapObject::Data1 {
        tag: invalid_tag,
        field: message,
    });
    let root = ctx
        .interpreter
        .process
        .temporary_roots
        .get_mut(checkpoint)
        .unwrap_or_else(|| solveig_base::ice!("bytesToString error root vanished"));
    *root = invalid;
    ctx.interpreter.safe_point();
    let invalid = temporary_root(ctx, checkpoint, "IOError");
    let result = ctx.interpreter.heap.alloc(HeapObject::Data1 {
        tag: result_tag,
        field: invalid,
    });
    ctx.interpreter.process.temporary_roots.truncate(checkpoint);
    Ok(result)
}

fn temporary_root(ctx: &VmCtx<'_, '_>, index: usize, label: &str) -> Value {
    ctx.interpreter
        .process
        .temporary_roots
        .get(index)
        .copied()
        .unwrap_or_else(|| solveig_base::ice!("bytesToString {label} root vanished"))
}

pub(super) fn bytes_length(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_bytes_owned(ctx, args, 0, "bytesLength")?;
    let length = i64::try_from(value.len())
        .map_err(|_range| RuntimeError::builtin("bytesLength exceeds Int range".to_string()))?;
    Ok(Value::Int(length))
}

pub(super) fn bytes_index(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_bytes_owned(ctx, args, 0, "bytesIndex")?;
    let index = expect_int(args, 1, "bytesIndex")?;
    let byte = usize::try_from(index)
        .ok()
        .and_then(|index| value.get(index))
        .copied()
        .map(|byte| Value::Int(i64::from(byte)));
    match byte {
        Some(byte) => ctx.alloc_data1_ctor(CtorRuntimeRole::PreludeMaybeJust, byte, "bytesIndex"),
        None => ctx.nullary_ctor(CtorRuntimeRole::PreludeMaybeNothing, "bytesIndex"),
    }
}

pub(super) fn bytes_slice(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_bytes_owned(ctx, args, 0, "bytesSlice")?;
    let start = expect_int(args, 1, "bytesSlice")?;
    let length = expect_int(args, 2, "bytesSlice")?;
    let start = usize::try_from(start).unwrap_or(0).min(value.len());
    let length = usize::try_from(length).unwrap_or(0);
    let end = start.saturating_add(length).min(value.len());
    let Some(slice) = value.get(start..end) else {
        solveig_base::ice!("clamped byte slice was outside its source")
    };
    Ok(ctx.alloc_bytes(slice))
}

pub(super) fn bytes_append(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let mut left = expect_bytes_owned(ctx, args, 0, "bytesAppend")?;
    let right = expect_bytes_owned(ctx, args, 1, "bytesAppend")?;
    left.extend_from_slice(&right);
    Ok(ctx.alloc_bytes(&left))
}

pub(super) fn expect_string_owned(
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

fn expect_bytes_owned(
    ctx: &VmCtx<'_, '_>,
    args: &[Value],
    index: usize,
    builtin: &str,
) -> Result<Vec<u8>, RuntimeError> {
    let value = expect_value(args, index, builtin)?;
    let Value::Obj(id) = value else {
        return Err(RuntimeError::builtin(format!(
            "{builtin} expects Bytes argument {index}"
        )));
    };
    let HeapObject::Bytes { bytes } = ctx.interpreter.heap.get(id) else {
        return Err(RuntimeError::builtin(format!(
            "{builtin} expects Bytes argument {index}"
        )));
    };
    Ok(ctx.interpreter.heap.bytes_value(id, *bytes).to_vec())
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
