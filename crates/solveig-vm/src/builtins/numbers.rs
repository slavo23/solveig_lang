//! Numeric primitive implementations and their documented edge-case semantics.

use super::{VmCtx, expect_float, expect_float2, expect_int, expect_int2, ordering_value};
use crate::values::Value;
use crate::vm::{DefectKind, RuntimeError};

// Int add/sub/mul/neg wrap on i64 overflow. Division and modulo defect on zero;
// i64::MIN divided/modded by -1 follows wrapping i64 behavior.
// Float arithmetic and comparisons delegate to f64, preserving IEEE-754 NaN
// and infinity behavior for arithmetic and relational predicates.

pub(crate) fn int_div_or_defect(lhs: i64, rhs: i64) -> Result<i64, RuntimeError> {
    if rhs == 0 {
        return Err(RuntimeError::defect(DefectKind::DivisionByZero));
    }
    if lhs == i64::MIN && rhs == -1 {
        return Ok(i64::MIN);
    }
    Ok(lhs / rhs)
}

pub(crate) fn int_mod_or_defect(lhs: i64, rhs: i64) -> Result<i64, RuntimeError> {
    if rhs == 0 {
        return Err(RuntimeError::defect(DefectKind::DivisionByZero));
    }
    if lhs == i64::MIN && rhs == -1 {
        return Ok(0);
    }
    Ok(lhs % rhs)
}

pub(crate) fn int_add(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intAdd")?;
    Ok(Value::Int(lhs.wrapping_add(rhs)))
}

pub(crate) fn int_sub(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intSub")?;
    Ok(Value::Int(lhs.wrapping_sub(rhs)))
}

pub(crate) fn int_mul(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intMul")?;
    Ok(Value::Int(lhs.wrapping_mul(rhs)))
}

pub(crate) fn int_div(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intDiv")?;
    Ok(Value::Int(int_div_or_defect(lhs, rhs)?))
}

pub(crate) fn int_mod(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intMod")?;
    Ok(Value::Int(int_mod_or_defect(lhs, rhs)?))
}

pub(crate) fn int_neg(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_int(args, 0, "intNeg")?;
    Ok(Value::Int(value.wrapping_neg()))
}

pub(crate) fn int_eq(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intEq")?;
    Ok(ctx.bool_value(lhs == rhs))
}

pub(crate) fn int_lt(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intLt")?;
    Ok(ctx.bool_value(lhs < rhs))
}

pub(crate) fn int_le(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intLe")?;
    Ok(ctx.bool_value(lhs <= rhs))
}

pub(crate) fn int_gt(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intGt")?;
    Ok(ctx.bool_value(lhs > rhs))
}

pub(crate) fn int_ge(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intGe")?;
    Ok(ctx.bool_value(lhs >= rhs))
}

pub(crate) fn int_compare(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intCompare")?;
    ordering_value(ctx, lhs.cmp(&rhs), "intCompare")
}

pub(crate) fn float_add(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatAdd")?;
    Ok(Value::Float(lhs + rhs))
}

pub(crate) fn float_sub(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatSub")?;
    Ok(Value::Float(lhs - rhs))
}

pub(crate) fn float_mul(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatMul")?;
    Ok(Value::Float(lhs * rhs))
}

pub(crate) fn float_div(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatDiv")?;
    Ok(Value::Float(lhs / rhs))
}

pub(crate) fn float_neg(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_float(args, 0, "floatNeg")?;
    Ok(Value::Float(-value))
}

pub(crate) fn float_eq(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatEq")?;
    Ok(ctx.bool_value(lhs == rhs))
}

pub(crate) fn float_lt(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatLt")?;
    Ok(ctx.bool_value(lhs < rhs))
}

pub(crate) fn float_le(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatLe")?;
    Ok(ctx.bool_value(lhs <= rhs))
}

pub(crate) fn float_gt(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatGt")?;
    Ok(ctx.bool_value(lhs > rhs))
}

pub(crate) fn float_ge(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatGe")?;
    Ok(ctx.bool_value(lhs >= rhs))
}

pub(crate) fn float_total_compare(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatTotalCompare")?;
    ordering_value(ctx, lhs.total_cmp(&rhs), "floatTotalCompare")
}

pub(crate) fn int_to_float(
    _ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_int(args, 0, "intToFloat")?;
    Ok(Value::Float(value as f64))
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "Solveig floatToInt is defined as Rust's saturating truncating f64-to-i64 conversion"
)]
pub(crate) fn float_to_int(
    _ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_float(args, 0, "floatToInt")?;
    // D3: truncate toward zero. Rust's f64 -> i64 cast is saturating and keeps
    // this conversion total for NaN and out-of-range values.
    Ok(Value::Int(value.trunc() as i64))
}
