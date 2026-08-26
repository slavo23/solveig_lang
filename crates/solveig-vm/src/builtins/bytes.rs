//! Immutable byte-string and UTF-8 boundary primitives.
//! This module owns packed byte scans, bulk construction, and explicit decoding.

use solveig_ast::ast::CtorRuntimeRole;

use super::text::{expect_string_id, string_ref};
use super::{VmCtx, byte_index_value, clamp_byte_index, expect_int, expect_value};
use crate::heap::HeapObject;
use crate::values::{ObjId, Value};
use crate::vm::RuntimeError;

pub(super) fn string_to_bytes(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let id = expect_string_id(ctx, args, 0, "stringToBytes")?;
    let bytes = {
        let byte_ref = string_ref(ctx, id, "stringToBytes")?;
        ctx.interpreter
            .heap
            .str_value(id, byte_ref)
            .as_bytes()
            .to_vec()
    };
    Ok(ctx.alloc_bytes(&bytes))
}

pub(super) fn bytes_to_string(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_bytes_owned(ctx, args, 0, "bytesToString")?;
    decode_utf8_result(ctx, value, "bytesToString")
}

pub(super) fn bytes_to_string_replacing(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let id = expect_bytes_id(ctx, args, 0, "bytesToStringReplacing")?;
    let text = {
        let byte_ref = bytes_ref(ctx, id, "bytesToStringReplacing")?;
        String::from_utf8_lossy(ctx.interpreter.heap.bytes_value(id, byte_ref)).into_owned()
    };
    Ok(ctx.alloc_str(&text))
}

pub(super) fn bytes_utf8_valid_prefix(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let bytes = expect_bytes_owned(ctx, args, 0, "bytesUtf8ValidPrefix")?;
    let end_of_input = match expect_value(args, 1, "bytesUtf8ValidPrefix")? {
        value if value == ctx.bool_value(true) => true,
        value if value == ctx.bool_value(false) => false,
        _ => {
            return Err(RuntimeError::builtin(
                "bytesUtf8ValidPrefix expected Bool argument at position 2".to_string(),
            ));
        }
    };
    let prefix = match std::str::from_utf8(&bytes) {
        Ok(_valid) => Ok(bytes.len()),
        Err(error) if error.error_len().is_none() && !end_of_input => Ok(error.valid_up_to()),
        Err(error) => Err(error.valid_up_to()),
    };

    match prefix {
        Ok(length) => {
            let length = i64::try_from(length).map_err(|_out_of_range| {
                RuntimeError::builtin("UTF-8 prefix length exceeds Int".to_string())
            })?;
            ctx.alloc_data1_ctor(
                CtorRuntimeRole::PreludeEitherRight,
                Value::Int(length),
                "bytesUtf8ValidPrefix",
            )
        }
        Err(offset) => alloc_invalid_utf8_result(ctx, offset, "bytesUtf8ValidPrefix"),
    }
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

pub(super) fn bytes_concat(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let values = expect_value(args, 0, "bytesConcat")?;
    let combined = concat_bytes_list(ctx, values, "bytesConcat")?;
    Ok(ctx.alloc_bytes(&combined))
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
    let accepted = byte_membership(ctx.interpreter.heap.bytes_value(accepted_id, accepted_ref));
    let start = clamp_byte_index(start, source.len());
    let tail = source
        .get(start..)
        .unwrap_or_else(|| solveig_base::ice!("clamped byte span start escaped its source"));
    let offset = tail
        .iter()
        .position(|byte| !byte_is_member(&accepted, *byte))
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
    let needles = byte_membership(ctx.interpreter.heap.bytes_value(needles_id, needles_ref));
    let start = clamp_byte_index(start, source.len());
    let tail = source
        .get(start..)
        .unwrap_or_else(|| solveig_base::ice!("clamped byte search start escaped its source"));
    let offset = tail
        .iter()
        .position(|byte| byte_is_member(&needles, *byte))
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
    let text = ctx.temporary_root(checkpoint, builtin, "text");
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
    let offset = ctx.temporary_root(checkpoint, builtin, "error offset");
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
    let invalid = ctx.temporary_root(checkpoint, builtin, "Utf8Error");
    let result = ctx.interpreter.heap.alloc(HeapObject::Data1 {
        tag: result_tag,
        field: invalid,
    });
    ctx.interpreter.process.temporary_roots.truncate(checkpoint);
    Ok(result)
}

fn byte_membership(bytes: &[u8]) -> [bool; 256] {
    let mut membership = [false; 256];
    for byte in bytes {
        let Some(slot) = membership.get_mut(usize::from(*byte)) else {
            solveig_base::ice!("u8 index escaped the 256-entry membership table")
        };
        *slot = true;
    }
    membership
}

fn byte_is_member(membership: &[bool; 256], byte: u8) -> bool {
    membership
        .get(usize::from(byte))
        .copied()
        .unwrap_or_else(|| solveig_base::ice!("u8 index escaped the 256-entry membership table"))
}

fn concat_bytes_list(
    ctx: &VmCtx<'_, '_>,
    mut current: Value,
    builtin: &str,
) -> Result<Vec<u8>, RuntimeError> {
    let nil_tag = ctx.ctor_tag_with_arity(CtorRuntimeRole::PreludeListNil, 0, builtin)?;
    let cons_tag = ctx.ctor_tag_with_arity(CtorRuntimeRole::PreludeListCons, 2, builtin)?;
    let mut output = Vec::new();

    loop {
        let Value::Obj(list_id) = current else {
            return Err(RuntimeError::builtin(format!(
                "{builtin} expected [Bytes] list spine"
            )));
        };
        match ctx.interpreter.heap.get(list_id) {
            HeapObject::Data0 { tag } if *tag == nil_tag => return Ok(output),
            HeapObject::Data2 {
                tag,
                field0,
                field1,
            } if *tag == cons_tag => {
                let Value::Obj(bytes_id) = field0 else {
                    return Err(RuntimeError::builtin(format!(
                        "{builtin} expected Bytes list element"
                    )));
                };
                let HeapObject::Bytes { bytes } = ctx.interpreter.heap.get(*bytes_id) else {
                    return Err(RuntimeError::builtin(format!(
                        "{builtin} expected Bytes list element"
                    )));
                };
                output.extend_from_slice(ctx.interpreter.heap.bytes_value(*bytes_id, *bytes));
                current = *field1;
            }
            _ => {
                return Err(RuntimeError::builtin(format!(
                    "{builtin} expected [Bytes] list spine"
                )));
            }
        }
    }
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
