//! Numeric primitive implementations and their documented edge-case semantics.

use super::{
    VmCtx, expect_float, expect_float2, expect_int, expect_int2, expect_value, ordering_value,
};
use crate::heap::HeapObject;
use crate::values::Value;
use crate::vm::{DefectKind, RuntimeError};
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{FromPrimitive, Signed, ToPrimitive, Zero};
use solveig_ast::ast::CtorRuntimeRole;

use super::text::expect_string_owned;

// Int arithmetic is checked. Division by zero and unrepresentable fixed-width
// results are structured defects. `div`/`mod` round toward negative infinity;
// `quot`/`rem` truncate toward zero.
// Float arithmetic and comparisons delegate to f64, preserving IEEE-754 NaN
// and infinity behavior for arithmetic and relational predicates.

pub(crate) fn int_div_or_defect(lhs: i64, rhs: i64) -> Result<i64, RuntimeError> {
    if rhs == 0 {
        return Err(RuntimeError::defect(DefectKind::DivisionByZero));
    }
    lhs.checked_div(rhs)
        .map(|quotient| {
            let remainder = lhs % rhs;
            if remainder != 0 && (remainder < 0) != (rhs < 0) {
                quotient - 1
            } else {
                quotient
            }
        })
        .ok_or_else(integer_overflow)
}

pub(crate) fn int_mod_or_defect(lhs: i64, rhs: i64) -> Result<i64, RuntimeError> {
    if rhs == 0 {
        return Err(RuntimeError::defect(DefectKind::DivisionByZero));
    }
    lhs.checked_rem(rhs)
        .map(|remainder| {
            if remainder != 0 && (remainder < 0) != (rhs < 0) {
                remainder + rhs
            } else {
                remainder
            }
        })
        .ok_or_else(integer_overflow)
}

fn integer_overflow() -> RuntimeError {
    RuntimeError::defect(DefectKind::IntegerOverflow)
}

fn negative_exponent() -> RuntimeError {
    RuntimeError::defect(DefectKind::Error(
        "negative exponent used with (^)".to_string(),
    ))
}

pub(crate) fn int_add(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intAdd")?;
    lhs.checked_add(rhs)
        .map(Value::Int)
        .ok_or_else(integer_overflow)
}

pub(crate) fn int_sub(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intSub")?;
    lhs.checked_sub(rhs)
        .map(Value::Int)
        .ok_or_else(integer_overflow)
}

pub(crate) fn int_mul(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intMul")?;
    lhs.checked_mul(rhs)
        .map(Value::Int)
        .ok_or_else(integer_overflow)
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
    value
        .checked_neg()
        .map(Value::Int)
        .ok_or_else(integer_overflow)
}

pub(crate) fn int_quot(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intQuot")?;
    if rhs == 0 {
        return Err(RuntimeError::defect(DefectKind::DivisionByZero));
    }
    lhs.checked_div(rhs)
        .map(Value::Int)
        .ok_or_else(integer_overflow)
}

pub(crate) fn int_rem(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intRem")?;
    if rhs == 0 {
        return Err(RuntimeError::defect(DefectKind::DivisionByZero));
    }
    lhs.checked_rem(rhs)
        .map(Value::Int)
        .ok_or_else(integer_overflow)
}

pub(crate) fn int_abs(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    expect_int(args, 0, "intAbs")?
        .checked_abs()
        .map(Value::Int)
        .ok_or_else(integer_overflow)
}

pub(crate) fn int_signum(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::Int(expect_int(args, 0, "intSignum")?.signum()))
}

pub(crate) fn int_mix_hash(
    _ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let (salt, value) = expect_int2(args, "intMixHash")?;
    Ok(Value::Int(
        salt.wrapping_mul(16_777_619).wrapping_add(value),
    ))
}

pub(crate) fn int_pow(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (mut base, exponent) = expect_int2(args, "intPow")?;
    let mut exponent = u64::try_from(exponent).map_err(|_negative_exponent| negative_exponent())?;
    let mut result = 1_i64;
    while exponent != 0 {
        if exponent & 1 == 1 {
            result = result.checked_mul(base).ok_or_else(integer_overflow)?;
        }
        exponent >>= 1;
        if exponent != 0 {
            base = base.checked_mul(base).ok_or_else(integer_overflow)?;
        }
    }
    Ok(Value::Int(result))
}

fn expect_big_int(
    ctx: &VmCtx<'_, '_>,
    args: &[Value],
    index: usize,
    builtin: &str,
) -> Result<BigInt, RuntimeError> {
    let Value::Obj(owner) = expect_value(args, index, builtin)? else {
        return Err(RuntimeError::builtin(format!(
            "{builtin} expected BigInt argument at position {}",
            index + 1
        )));
    };
    let HeapObject::BigInt { value } = *ctx.interpreter.heap.get(owner) else {
        return Err(RuntimeError::builtin(format!(
            "{builtin} expected BigInt argument at position {}",
            index + 1
        )));
    };
    Ok(ctx.interpreter.heap.big_int_value(owner, value).clone())
}

fn expect_big_int2(
    ctx: &VmCtx<'_, '_>,
    args: &[Value],
    builtin: &str,
) -> Result<(BigInt, BigInt), RuntimeError> {
    Ok((
        expect_big_int(ctx, args, 0, builtin)?,
        expect_big_int(ctx, args, 1, builtin)?,
    ))
}

pub(crate) fn big_int_from_int(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_big_int(BigInt::from(expect_int(args, 0, "bigIntFromInt")?)))
}

pub(crate) fn big_int_to_int(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    match expect_big_int(ctx, args, 0, "bigIntToInt")?.to_i64() {
        Some(value) => ctx.alloc_data1_ctor(
            solveig_ast::ast::CtorRuntimeRole::PreludeMaybeJust,
            Value::Int(value),
            "bigIntToInt",
        ),
        None => ctx.nullary_ctor(
            solveig_ast::ast::CtorRuntimeRole::PreludeMaybeNothing,
            "bigIntToInt",
        ),
    }
}

pub(crate) fn big_int_to_float(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_big_int(ctx, args, 0, "bigIntToFloat")?;
    Ok(Value::Float(value.to_f64().unwrap_or_else(|| {
        if value.is_negative() {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        }
    })))
}

pub(crate) fn big_int_to_string(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_big_int(ctx, args, 0, "bigIntToString")?;
    Ok(ctx.alloc_str(&value.to_string()))
}

macro_rules! big_int_binary {
    ($name:ident, $label:literal, $op:tt) => {
        pub(crate) fn $name(
            ctx: &mut VmCtx<'_, '_>,
            args: &[Value],
        ) -> Result<Value, RuntimeError> {
            let (lhs, rhs) = expect_big_int2(ctx, args, $label)?;
            Ok(ctx.alloc_big_int(lhs $op rhs))
        }
    };
}

big_int_binary!(big_int_add, "bigIntAdd", +);
big_int_binary!(big_int_sub, "bigIntSub", -);
big_int_binary!(big_int_mul, "bigIntMul", *);

pub(crate) fn big_int_neg(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_big_int(-expect_big_int(ctx, args, 0, "bigIntNeg")?))
}

pub(crate) fn big_int_abs(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_big_int(expect_big_int(ctx, args, 0, "bigIntAbs")?.abs()))
}

pub(crate) fn big_int_signum(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_big_int(expect_big_int(ctx, args, 0, "bigIntSignum")?.signum()))
}

fn checked_big_int_divisor(rhs: &BigInt) -> Result<(), RuntimeError> {
    if rhs.is_zero() {
        Err(RuntimeError::defect(DefectKind::DivisionByZero))
    } else {
        Ok(())
    }
}

pub(crate) fn big_int_div(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_big_int2(ctx, args, "bigIntDiv")?;
    checked_big_int_divisor(&rhs)?;
    Ok(ctx.alloc_big_int(lhs.div_floor(&rhs)))
}

pub(crate) fn big_int_mod(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_big_int2(ctx, args, "bigIntMod")?;
    checked_big_int_divisor(&rhs)?;
    Ok(ctx.alloc_big_int(lhs.mod_floor(&rhs)))
}

pub(crate) fn big_int_quot(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_big_int2(ctx, args, "bigIntQuot")?;
    checked_big_int_divisor(&rhs)?;
    Ok(ctx.alloc_big_int(lhs / rhs))
}

pub(crate) fn big_int_rem(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_big_int2(ctx, args, "bigIntRem")?;
    checked_big_int_divisor(&rhs)?;
    Ok(ctx.alloc_big_int(lhs % rhs))
}

pub(crate) fn big_int_pow(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let base = expect_big_int(ctx, args, 0, "bigIntPow")?;
    let exponent = expect_int(args, 1, "bigIntPow")?;
    let exponent = u32::try_from(exponent).map_err(|_negative_exponent| negative_exponent())?;
    Ok(ctx.alloc_big_int(base.pow(exponent)))
}

pub(crate) fn big_int_eq(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_big_int2(ctx, args, "bigIntEq")?;
    Ok(ctx.bool_value(lhs == rhs))
}

pub(crate) fn big_int_compare(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_big_int2(ctx, args, "bigIntCompare")?;
    ordering_value(ctx, lhs.cmp(&rhs), "bigIntCompare")
}

fn alloc_maybe_big_int(
    ctx: &mut VmCtx<'_, '_>,
    value: Option<BigInt>,
    builtin: &str,
) -> Result<Value, RuntimeError> {
    let Some(value) = value else {
        return ctx.nullary_ctor(CtorRuntimeRole::PreludeMaybeNothing, builtin);
    };
    let tag = ctx.ctor_tag_with_arity(CtorRuntimeRole::PreludeMaybeJust, 1, builtin)?;
    let checkpoint = ctx.interpreter.process.temporary_roots.len();
    let value = ctx.alloc_big_int(value);
    ctx.interpreter.process.temporary_roots.push(value);
    ctx.interpreter.safe_point();
    let value = ctx.temporary_root(checkpoint, builtin, "BigInt");
    let result = ctx
        .interpreter
        .heap
        .alloc(HeapObject::Data1 { tag, field: value });
    ctx.interpreter.process.temporary_roots.truncate(checkpoint);
    Ok(result)
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
    let value = ctx.alloc_str(&value);
    ctx.interpreter.process.temporary_roots.push(value);
    ctx.interpreter.safe_point();
    let value = ctx.temporary_root(checkpoint, builtin, "String");
    let result = ctx
        .interpreter
        .heap
        .alloc(HeapObject::Data1 { tag, field: value });
    ctx.interpreter.process.temporary_roots.truncate(checkpoint);
    Ok(result)
}

fn radix(value: i64) -> Option<u32> {
    u32::try_from(value)
        .ok()
        .filter(|radix| (2..=36).contains(radix))
}

pub(crate) fn big_int_parse(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let text = expect_string_owned(ctx, args, 0, "bigIntParse")?;
    alloc_maybe_big_int(ctx, BigInt::parse_bytes(text.as_bytes(), 10), "bigIntParse")
}

pub(crate) fn big_int_parse_base(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let radix = radix(expect_int(args, 0, "bigIntParseBase")?);
    let text = expect_string_owned(ctx, args, 1, "bigIntParseBase")?;
    let value = radix.and_then(|radix| BigInt::parse_bytes(text.as_bytes(), radix));
    alloc_maybe_big_int(ctx, value, "bigIntParseBase")
}

pub(crate) fn big_int_to_string_base(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let radix = radix(expect_int(args, 0, "bigIntToStringBase")?);
    let value = expect_big_int(ctx, args, 1, "bigIntToStringBase")?;
    alloc_maybe_string(
        ctx,
        radix.map(|radix| value.to_str_radix(radix)),
        "bigIntToStringBase",
    )
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

pub(crate) fn float_abs(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::Float(libm::fabs(expect_float(args, 0, "floatAbs")?)))
}

pub(crate) fn float_signum(
    _ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_float(args, 0, "floatSignum")?;
    let signum = if value == 0.0 || value.is_nan() {
        value
    } else {
        libm::copysign(1.0, value)
    };
    Ok(Value::Float(signum))
}

macro_rules! float_unary {
    ($name:ident, $label:literal, $function:path) => {
        pub(crate) fn $name(
            _ctx: &mut VmCtx<'_, '_>,
            args: &[Value],
        ) -> Result<Value, RuntimeError> {
            Ok(Value::Float($function(expect_float(args, 0, $label)?)))
        }
    };
}

macro_rules! float_binary {
    ($name:ident, $label:literal, $function:path) => {
        pub(crate) fn $name(
            _ctx: &mut VmCtx<'_, '_>,
            args: &[Value],
        ) -> Result<Value, RuntimeError> {
            let (lhs, rhs) = expect_float2(args, $label)?;
            Ok(Value::Float($function(lhs, rhs)))
        }
    };
}

float_binary!(float_pow, "floatPow", libm::pow);
float_unary!(float_sqrt, "floatSqrt", libm::sqrt);
float_unary!(float_cbrt, "floatCbrt", libm::cbrt);
float_binary!(float_hypot, "floatHypot", libm::hypot);
float_unary!(float_exp, "floatExp", libm::exp);
float_unary!(float_exp2, "floatExp2", libm::exp2);
float_unary!(float_log, "floatLog", libm::log);
float_unary!(float_log2, "floatLog2", libm::log2);
float_unary!(float_log10, "floatLog10", libm::log10);
float_unary!(float_sin, "floatSin", libm::sin);
float_unary!(float_cos, "floatCos", libm::cos);
float_unary!(float_tan, "floatTan", libm::tan);
float_unary!(float_asin, "floatAsin", libm::asin);
float_unary!(float_acos, "floatAcos", libm::acos);
float_unary!(float_atan, "floatAtan", libm::atan);
float_binary!(float_atan2, "floatAtan2", libm::atan2);
float_unary!(float_sinh, "floatSinh", libm::sinh);
float_unary!(float_cosh, "floatCosh", libm::cosh);
float_unary!(float_tanh, "floatTanh", libm::tanh);
float_unary!(float_floor, "floatFloor", libm::floor);
float_unary!(float_ceil, "floatCeil", libm::ceil);
float_unary!(float_truncate, "floatTruncate", libm::trunc);
float_unary!(float_round, "floatRound", libm::round);
float_binary!(float_copy_sign, "floatCopySign", libm::copysign);

pub(crate) fn float_is_nan(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(ctx.bool_value(expect_float(args, 0, "floatIsNaN")?.is_nan()))
}

pub(crate) fn float_is_infinite(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    Ok(ctx.bool_value(expect_float(args, 0, "floatIsInfinite")?.is_infinite()))
}

pub(crate) fn float_is_finite(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    Ok(ctx.bool_value(expect_float(args, 0, "floatIsFinite")?.is_finite()))
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

pub(crate) fn float_to_int_maybe(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_float(args, 0, "floatToIntMaybe")?;
    if value.is_finite() && value >= i64::MIN as f64 && value < -(i64::MIN as f64) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "bounds and finiteness are checked before the truncating conversion"
        )]
        let value = Value::Int(value.trunc() as i64);
        ctx.alloc_data1_ctor(CtorRuntimeRole::PreludeMaybeJust, value, "floatToIntMaybe")
    } else {
        ctx.nullary_ctor(CtorRuntimeRole::PreludeMaybeNothing, "floatToIntMaybe")
    }
}

pub(crate) fn float_to_big_int(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let value = expect_float(args, 0, "floatToBigInt")?;
    alloc_maybe_big_int(ctx, BigInt::from_f64(value.trunc()), "floatToBigInt")
}
