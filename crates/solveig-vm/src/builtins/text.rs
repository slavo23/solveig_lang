//! Packed text and character primitives.
//! This module owns scalar/byte indexing, scans, and GC-safe text construction.

use solveig_ast::ast::CtorRuntimeRole;

use super::{
    VmCtx, byte_index_value, clamp_byte_index, expect_char, expect_char2, expect_float, expect_int,
    expect_value, ordering_value,
};
use crate::heap::HeapObject;
use crate::values::{ObjId, Value};
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

pub(super) fn char_to_code_point(
    _ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_char(args, 0, "charToCodePoint")?;
    Ok(Value::Int(i64::from(u32::from(value))))
}

pub(super) fn code_point_to_string(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let code_point = expect_int(args, 0, "codePointToString")?;
    let character = u32::try_from(code_point).ok().and_then(char::from_u32);
    match character {
        Some(character) => {
            let tag =
                ctx.ctor_tag_with_arity(CtorRuntimeRole::PreludeMaybeJust, 1, "codePointToString")?;
            let checkpoint = ctx.interpreter.process.temporary_roots.len();
            let text = ctx.alloc_str(&character.to_string());
            ctx.interpreter.process.temporary_roots.push(text);
            ctx.interpreter.safe_point();
            let text = ctx.temporary_root(checkpoint, "codePointToString", "text");
            let result = ctx
                .interpreter
                .heap
                .alloc(HeapObject::Data1 { tag, field: text });
            ctx.interpreter.process.temporary_roots.truncate(checkpoint);
            Ok(result)
        }
        None => ctx.nullary_ctor(CtorRuntimeRole::PreludeMaybeNothing, "codePointToString"),
    }
}

pub(super) fn string_length(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let id = expect_string_id(ctx, args, 0, "stringLength")?;
    let byte_ref = string_ref(ctx, id, "stringLength")?;
    let value = ctx.interpreter.heap.str_value(id, byte_ref);
    let len = i64::try_from(value.chars().count()).map_err(|_error| {
        RuntimeError::builtin("stringLength result does not fit in Int".to_string())
    })?;
    Ok(Value::Int(len))
}

pub(super) fn string_byte_length(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let id = expect_string_id(ctx, args, 0, "stringByteLength")?;
    let byte_ref = string_ref(ctx, id, "stringByteLength")?;
    byte_index_value(byte_ref.len(), "stringByteLength")
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
    let id = expect_string_id(ctx, args, 0, "stringSlice")?;
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
    let sliced = {
        let byte_ref = string_ref(ctx, id, "stringSlice")?;
        ctx.interpreter
            .heap
            .str_value(id, byte_ref)
            .chars()
            .skip(start)
            .take(len)
            .collect::<String>()
    };
    Ok(ctx.alloc_str(&sliced))
}

pub(super) fn string_char_at(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let id = expect_string_id(ctx, args, 0, "stringCharAt")?;
    let index = expect_int(args, 1, "stringCharAt")?;
    let Some(index) = usize::try_from(index).ok() else {
        return ctx.nullary_ctor(CtorRuntimeRole::PreludeMaybeNothing, "stringCharAt");
    };

    let character = {
        let byte_ref = string_ref(ctx, id, "stringCharAt")?;
        ctx.interpreter
            .heap
            .str_value(id, byte_ref)
            .chars()
            .nth(index)
    };
    match character {
        Some(ch) => ctx.alloc_data1_ctor(
            CtorRuntimeRole::PreludeMaybeJust,
            Value::Char(ch),
            "stringCharAt",
        ),
        None => ctx.nullary_ctor(CtorRuntimeRole::PreludeMaybeNothing, "stringCharAt"),
    }
}

pub(super) fn string_eq(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let lhs_id = expect_string_id(ctx, args, 0, "stringEq")?;
    let rhs_id = expect_string_id(ctx, args, 1, "stringEq")?;
    let lhs_ref = string_ref(ctx, lhs_id, "stringEq")?;
    let rhs_ref = string_ref(ctx, rhs_id, "stringEq")?;
    let lhs = ctx.interpreter.heap.str_value(lhs_id, lhs_ref);
    let rhs = ctx.interpreter.heap.str_value(rhs_id, rhs_ref);
    Ok(ctx.bool_value(lhs == rhs))
}

pub(super) fn string_compare(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let lhs_id = expect_string_id(ctx, args, 0, "stringCompare")?;
    let rhs_id = expect_string_id(ctx, args, 1, "stringCompare")?;
    let lhs_ref = string_ref(ctx, lhs_id, "stringCompare")?;
    let rhs_ref = string_ref(ctx, rhs_id, "stringCompare")?;
    let lhs = ctx.interpreter.heap.str_value(lhs_id, lhs_ref);
    let rhs = ctx.interpreter.heap.str_value(rhs_id, rhs_ref);
    ordering_value(ctx, lhs.cmp(rhs), "stringCompare")
}

pub(super) fn string_starts_with(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value_id = expect_string_id(ctx, args, 0, "stringStartsWith")?;
    let prefix_id = expect_string_id(ctx, args, 1, "stringStartsWith")?;
    let value_ref = string_ref(ctx, value_id, "stringStartsWith")?;
    let prefix_ref = string_ref(ctx, prefix_id, "stringStartsWith")?;
    let value = ctx.interpreter.heap.str_value(value_id, value_ref);
    let prefix = ctx.interpreter.heap.str_value(prefix_id, prefix_ref);
    Ok(ctx.bool_value(value.starts_with(prefix)))
}

pub(super) fn string_find_char_from(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let id = expect_string_id(ctx, args, 0, "stringFindCharFrom")?;
    let start = expect_int(args, 1, "stringFindCharFrom")?;
    let needle = expect_char(args, 2, "stringFindCharFrom")?;
    let byte_ref = string_ref(ctx, id, "stringFindCharFrom")?;
    let value = ctx.interpreter.heap.str_value(id, byte_ref);
    let start = clamp_byte_index(start, value.len());
    let index = value
        .get(start..)
        .and_then(|tail| tail.find(needle).map(|offset| start + offset))
        .unwrap_or(value.len());
    byte_index_value(index, "stringFindCharFrom")
}

pub(super) fn string_byte_slice(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let id = expect_string_id(ctx, args, 0, "stringByteSlice")?;
    let start = expect_int(args, 1, "stringByteSlice")?;
    let length = expect_int(args, 2, "stringByteSlice")?;
    let byte_ref = string_ref(ctx, id, "stringByteSlice")?;
    let value = ctx.interpreter.heap.str_value(id, byte_ref);
    let slice = usize::try_from(start)
        .ok()
        .zip(usize::try_from(length).ok())
        .and_then(|(start, length)| start.checked_add(length).map(|end| (start, end)))
        .and_then(|(start, end)| value.get(start..end).map(str::to_owned));
    alloc_maybe_string(ctx, slice, "stringByteSlice")
}

pub(super) fn string_trim_ascii(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let id = expect_string_id(ctx, args, 0, "stringTrimAscii")?;
    let trimmed = {
        let byte_ref = string_ref(ctx, id, "stringTrimAscii")?;
        ctx.interpreter
            .heap
            .str_value(id, byte_ref)
            .trim_matches(|character| matches!(character, ' ' | '\t' | '\r' | '\n'))
            .to_owned()
    };
    Ok(ctx.alloc_str(&trimmed))
}

pub(super) fn strings_concat(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let values = expect_value(args, 0, "stringsConcat")?;
    let combined = concat_string_list(ctx, values, "stringsConcat")?;
    Ok(ctx.alloc_str(&combined))
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
    let id = expect_string_id(ctx, args, 0, "stringToChars")?;
    let chars = {
        let byte_ref = string_ref(ctx, id, "stringToChars")?;
        ctx.interpreter
            .heap
            .str_value(id, byte_ref)
            .chars()
            .map(Value::Char)
            .collect::<Vec<_>>()
    };
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

fn alloc_maybe_string(
    ctx: &mut VmCtx<'_, '_>,
    value: Option<String>,
    builtin: &str,
) -> Result<Value, RuntimeError> {
    let Some(value) = value else {
        return ctx.nullary_ctor(CtorRuntimeRole::PreludeMaybeNothing, builtin);
    };
    let tag = ctx.ctor_tag_with_arity(CtorRuntimeRole::PreludeMaybeJust, 1, builtin)?;
    let checkpoint = ctx.interpreter.process.temporary_roots.len();
    let text = ctx.alloc_str(&value);
    ctx.interpreter.process.temporary_roots.push(text);
    ctx.interpreter.safe_point();
    let text = ctx.temporary_root(checkpoint, builtin, "text");
    let result = ctx
        .interpreter
        .heap
        .alloc(HeapObject::Data1 { tag, field: text });
    ctx.interpreter.process.temporary_roots.truncate(checkpoint);
    Ok(result)
}

fn concat_string_list(
    ctx: &VmCtx<'_, '_>,
    mut current: Value,
    builtin: &str,
) -> Result<String, RuntimeError> {
    let nil_tag = ctx.ctor_tag_with_arity(CtorRuntimeRole::PreludeListNil, 0, builtin)?;
    let cons_tag = ctx.ctor_tag_with_arity(CtorRuntimeRole::PreludeListCons, 2, builtin)?;
    let mut output = String::new();

    loop {
        let Value::Obj(list_id) = current else {
            return Err(RuntimeError::builtin(format!(
                "{builtin} expected [String] list spine"
            )));
        };
        match ctx.interpreter.heap.get(list_id) {
            HeapObject::Data0 { tag } if *tag == nil_tag => return Ok(output),
            HeapObject::Data2 {
                tag,
                field0,
                field1,
            } if *tag == cons_tag => {
                let Value::Obj(string_id) = field0 else {
                    return Err(RuntimeError::builtin(format!(
                        "{builtin} expected String list element"
                    )));
                };
                let HeapObject::Str { bytes } = ctx.interpreter.heap.get(*string_id) else {
                    return Err(RuntimeError::builtin(format!(
                        "{builtin} expected String list element"
                    )));
                };
                output.push_str(ctx.interpreter.heap.str_value(*string_id, *bytes));
                current = *field1;
            }
            _ => {
                return Err(RuntimeError::builtin(format!(
                    "{builtin} expected [String] list spine"
                )));
            }
        }
    }
}

pub(super) fn expect_string_id(
    ctx: &VmCtx<'_, '_>,
    args: &[Value],
    index: usize,
    builtin: &str,
) -> Result<ObjId, RuntimeError> {
    match expect_value(args, index, builtin)? {
        Value::Obj(id) if matches!(ctx.interpreter.heap.get(id), HeapObject::Str { .. }) => Ok(id),
        _ => Err(RuntimeError::builtin(format!(
            "{builtin} expects String argument {}",
            index + 1
        ))),
    }
}

pub(super) fn string_ref(
    ctx: &VmCtx<'_, '_>,
    id: ObjId,
    builtin: &str,
) -> Result<crate::heap::ByteRef, RuntimeError> {
    match ctx.interpreter.heap.get(id) {
        HeapObject::Str { bytes } => Ok(*bytes),
        _ => Err(RuntimeError::builtin(format!(
            "{builtin} expected a validated String object"
        ))),
    }
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
