//! Text, character, and immutable byte-string primitives.
//! This module owns conversion semantics and GC-safe construction of results.

use solveig_ast::ast::CtorRuntimeRole;

use super::{
    VmCtx, expect_char, expect_char2, expect_float, expect_int, expect_value, ordering_value,
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
            let text = temporary_root(ctx, checkpoint, "codePointToString", "text");
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
    decode_utf8_result(ctx, value, "bytesToString")
}

fn decode_utf8_result(
    ctx: &mut VmCtx<'_, '_>,
    value: Vec<u8>,
    builtin: &str,
) -> Result<Value, RuntimeError> {
    match String::from_utf8(value) {
        Ok(text) => alloc_valid_utf8_result(ctx, &text, builtin),
        Err(error) => alloc_invalid_utf8_result(ctx, error.utf8_error().valid_up_to(), builtin),
    }
}

fn alloc_valid_utf8_result(
    ctx: &mut VmCtx<'_, '_>,
    text: &str,
    builtin: &str,
) -> Result<Value, RuntimeError> {
    let tag = ctx.ctor_tag_with_arity(CtorRuntimeRole::PreludeEitherRight, 1, builtin)?;
    let checkpoint = ctx.interpreter.process.temporary_roots.len();
    let text = ctx.alloc_str(text);
    ctx.interpreter.process.temporary_roots.push(text);
    ctx.interpreter.safe_point();
    let text = temporary_root(ctx, checkpoint, builtin, "text");
    let result = ctx
        .interpreter
        .heap
        .alloc(HeapObject::Data1 { tag, field: text });
    ctx.interpreter.process.temporary_roots.truncate(checkpoint);
    Ok(result)
}

fn alloc_invalid_utf8_result(
    ctx: &mut VmCtx<'_, '_>,
    offset: usize,
    builtin: &str,
) -> Result<Value, RuntimeError> {
    let invalid_tag =
        ctx.ctor_tag_with_arity(CtorRuntimeRole::PreludeUtf8ErrorInvalid, 1, builtin)?;
    let result_tag = ctx.ctor_tag_with_arity(CtorRuntimeRole::PreludeEitherLeft, 1, builtin)?;
    let offset = i64::try_from(offset)
        .map_err(|_out_of_range| RuntimeError::builtin("UTF-8 offset exceeds Int".to_string()))?;
    let checkpoint = ctx.interpreter.process.temporary_roots.len();
    ctx.interpreter
        .process
        .temporary_roots
        .push(Value::Int(offset));
    ctx.interpreter.safe_point();
    let offset = temporary_root(ctx, checkpoint, builtin, "error offset");
    let invalid = ctx.interpreter.heap.alloc(HeapObject::Data1 {
        tag: invalid_tag,
        field: offset,
    });
    let root = ctx
        .interpreter
        .process
        .temporary_roots
        .get_mut(checkpoint)
        .unwrap_or_else(|| solveig_base::ice!("{builtin} error root vanished"));
    *root = invalid;
    ctx.interpreter.safe_point();
    let invalid = temporary_root(ctx, checkpoint, builtin, "Utf8Error");
    let result = ctx.interpreter.heap.alloc(HeapObject::Data1 {
        tag: result_tag,
        field: invalid,
    });
    ctx.interpreter.process.temporary_roots.truncate(checkpoint);
    Ok(result)
}

fn temporary_root(ctx: &VmCtx<'_, '_>, index: usize, builtin: &str, label: &str) -> Value {
    ctx.interpreter
        .process
        .temporary_roots
        .get(index)
        .copied()
        .unwrap_or_else(|| solveig_base::ice!("{builtin} {label} root vanished"))
}

pub(super) fn bytes_length(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let id = expect_bytes_id(ctx, args, 0, "bytesLength")?;
    let byte_ref = bytes_ref(ctx, id, "bytesLength")?;
    let length = i64::try_from(ctx.interpreter.heap.bytes_value(id, byte_ref).len())
        .map_err(|_range| RuntimeError::builtin("bytesLength exceeds Int range".to_string()))?;
    Ok(Value::Int(length))
}

pub(super) fn bytes_index(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let id = expect_bytes_id(ctx, args, 0, "bytesIndex")?;
    let index = expect_int(args, 1, "bytesIndex")?;
    let byte_ref = bytes_ref(ctx, id, "bytesIndex")?;
    let byte = usize::try_from(index)
        .ok()
        .and_then(|index| ctx.interpreter.heap.bytes_value(id, byte_ref).get(index))
        .copied()
        .map(|byte| Value::Int(i64::from(byte)));
    match byte {
        Some(byte) => ctx.alloc_data1_ctor(CtorRuntimeRole::PreludeMaybeJust, byte, "bytesIndex"),
        None => ctx.nullary_ctor(CtorRuntimeRole::PreludeMaybeNothing, "bytesIndex"),
    }
}

pub(super) fn bytes_slice(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let id = expect_bytes_id(ctx, args, 0, "bytesSlice")?;
    let start = expect_int(args, 1, "bytesSlice")?;
    let length = expect_int(args, 2, "bytesSlice")?;
    let slice = {
        let byte_ref = bytes_ref(ctx, id, "bytesSlice")?;
        let value = ctx.interpreter.heap.bytes_value(id, byte_ref);
        let start = usize::try_from(start).unwrap_or(0).min(value.len());
        let length = usize::try_from(length).unwrap_or(0);
        let end = start.saturating_add(length).min(value.len());
        let Some(slice) = value.get(start..end) else {
            solveig_base::ice!("clamped byte slice was outside its source")
        };
        slice.to_vec()
    };
    Ok(ctx.alloc_bytes(&slice))
}

pub(super) fn bytes_append(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let mut left = expect_bytes_owned(ctx, args, 0, "bytesAppend")?;
    let right = expect_bytes_owned(ctx, args, 1, "bytesAppend")?;
    left.extend_from_slice(&right);
    Ok(ctx.alloc_bytes(&left))
}

pub(super) fn bytes_span_any(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let source_id = expect_bytes_id(ctx, args, 0, "bytesSpanAny")?;
    let start = expect_int(args, 1, "bytesSpanAny")?;
    let accepted_id = expect_bytes_id(ctx, args, 2, "bytesSpanAny")?;
    let source_ref = bytes_ref(ctx, source_id, "bytesSpanAny")?;
    let accepted_ref = bytes_ref(ctx, accepted_id, "bytesSpanAny")?;
    let source = ctx.interpreter.heap.bytes_value(source_id, source_ref);
    let accepted = ctx.interpreter.heap.bytes_value(accepted_id, accepted_ref);
    let start = clamp_byte_index(start, source.len());
    let tail = source
        .get(start..)
        .unwrap_or_else(|| solveig_base::ice!("clamped byte span start escaped its source"));
    let offset = tail
        .iter()
        .position(|byte| !accepted.contains(byte))
        .map_or(source.len(), |offset| start + offset);
    byte_index_value(offset, "bytesSpanAny")
}

pub(super) fn bytes_find_any(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let source_id = expect_bytes_id(ctx, args, 0, "bytesFindAny")?;
    let start = expect_int(args, 1, "bytesFindAny")?;
    let needles_id = expect_bytes_id(ctx, args, 2, "bytesFindAny")?;
    let source_ref = bytes_ref(ctx, source_id, "bytesFindAny")?;
    let needles_ref = bytes_ref(ctx, needles_id, "bytesFindAny")?;
    let source = ctx.interpreter.heap.bytes_value(source_id, source_ref);
    let needles = ctx.interpreter.heap.bytes_value(needles_id, needles_ref);
    let start = clamp_byte_index(start, source.len());
    let tail = source
        .get(start..)
        .unwrap_or_else(|| solveig_base::ice!("clamped byte search start escaped its source"));
    let offset = tail
        .iter()
        .position(|byte| needles.contains(byte))
        .map_or(source.len(), |offset| start + offset);
    byte_index_value(offset, "bytesFindAny")
}

pub(super) fn bytes_find_below(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let source_id = expect_bytes_id(ctx, args, 0, "bytesFindBelow")?;
    let start = expect_int(args, 1, "bytesFindBelow")?;
    let limit = expect_int(args, 2, "bytesFindBelow")?;
    let source_ref = bytes_ref(ctx, source_id, "bytesFindBelow")?;
    let source = ctx.interpreter.heap.bytes_value(source_id, source_ref);
    let start = clamp_byte_index(start, source.len());
    let tail = source
        .get(start..)
        .unwrap_or_else(|| solveig_base::ice!("clamped byte search start escaped its source"));
    let offset = tail
        .iter()
        .position(|byte| i64::from(*byte) < limit)
        .map_or(source.len(), |offset| start + offset);
    byte_index_value(offset, "bytesFindBelow")
}

pub(super) fn bytes_span_range(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let source_id = expect_bytes_id(ctx, args, 0, "bytesSpanRange")?;
    let start = expect_int(args, 1, "bytesSpanRange")?;
    let lower = expect_int(args, 2, "bytesSpanRange")?;
    let upper = expect_int(args, 3, "bytesSpanRange")?;
    let source_ref = bytes_ref(ctx, source_id, "bytesSpanRange")?;
    let source = ctx.interpreter.heap.bytes_value(source_id, source_ref);
    let start = clamp_byte_index(start, source.len());
    let tail = source
        .get(start..)
        .unwrap_or_else(|| solveig_base::ice!("clamped byte span start escaped its source"));
    let offset = tail
        .iter()
        .position(|byte| {
            let byte = i64::from(*byte);
            byte < lower || byte > upper
        })
        .map_or(source.len(), |offset| start + offset);
    byte_index_value(offset, "bytesSpanRange")
}

pub(super) fn bytes_slice_to_string(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let source_id = expect_bytes_id(ctx, args, 0, "bytesSliceToString")?;
    let start = expect_int(args, 1, "bytesSliceToString")?;
    let length = expect_int(args, 2, "bytesSliceToString")?;
    let slice = {
        let source_ref = bytes_ref(ctx, source_id, "bytesSliceToString")?;
        let source = ctx.interpreter.heap.bytes_value(source_id, source_ref);
        let start = clamp_byte_index(start, source.len());
        let length = usize::try_from(length).unwrap_or(0);
        let end = start.saturating_add(length).min(source.len());
        let Some(slice) = source.get(start..end) else {
            solveig_base::ice!("clamped UTF-8 byte slice was outside its source")
        };
        slice.to_vec()
    };
    decode_utf8_result(ctx, slice, "bytesSliceToString")
}

fn clamp_byte_index(index: i64, length: usize) -> usize {
    usize::try_from(index).unwrap_or(0).min(length)
}

fn byte_index_value(index: usize, builtin: &str) -> Result<Value, RuntimeError> {
    i64::try_from(index)
        .map(Value::Int)
        .map_err(|_error| RuntimeError::builtin(format!("{builtin} result does not fit in Int")))
}

fn expect_bytes_id(
    ctx: &VmCtx<'_, '_>,
    args: &[Value],
    index: usize,
    builtin: &str,
) -> Result<ObjId, RuntimeError> {
    match expect_value(args, index, builtin)? {
        Value::Obj(id) if matches!(ctx.interpreter.heap.get(id), HeapObject::Bytes { .. }) => {
            Ok(id)
        }
        _ => Err(RuntimeError::builtin(format!(
            "{builtin} expects Bytes argument {}",
            index + 1
        ))),
    }
}

fn bytes_ref(
    ctx: &VmCtx<'_, '_>,
    id: ObjId,
    builtin: &str,
) -> Result<crate::heap::ByteRef, RuntimeError> {
    match ctx.interpreter.heap.get(id) {
        HeapObject::Bytes { bytes } => Ok(*bytes),
        _ => Err(RuntimeError::builtin(format!(
            "{builtin} expected a validated Bytes object"
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
