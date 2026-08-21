//! Pure SplitMix generator operations and fresh-generator action construction.

use super::*;
use crate::StdGen;

pub(super) fn mk_std_gen(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let seed = expect_int(args, 0, "mkStdGen")?;
    Ok(Value::StdGen(StdGen::from_seed(seed)))
}

pub(super) fn new_std_gen(ctx: &mut VmCtx<'_, '_>, _args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_io_action(IoAction::NewStdGen))
}

pub(super) fn random_int(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let generator = expect_std_gen(args, 0, "randomInt")?;
    let (value, next) = generator.next_int();
    ctx.alloc_data2_ctor(
        CtorRuntimeRole::PreludeTuple(2),
        Value::Int(value),
        Value::StdGen(next),
        "randomInt",
    )
}

pub(super) fn random_range(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let lower = expect_int(args, 0, "randomRange")?;
    let upper = expect_int(args, 1, "randomRange")?;
    let generator = expect_std_gen(args, 2, "randomRange")?;
    let Some((value, next)) = generator.next_range(lower, upper) else {
        return Err(RuntimeError::defect(DefectKind::Error(
            "randomRange lower bound must be less than or equal to upper bound".to_string(),
        )));
    };
    ctx.alloc_data2_ctor(
        CtorRuntimeRole::PreludeTuple(2),
        Value::Int(value),
        Value::StdGen(next),
        "randomRange",
    )
}

fn expect_std_gen(
    args: &[Value],
    index: usize,
    builtin: &'static str,
) -> Result<StdGen, RuntimeError> {
    match expect_value(args, index, builtin)? {
        Value::StdGen(generator) => Ok(generator),
        _ => Err(RuntimeError::builtin(format!(
            "{builtin} expected StdGen at argument {index}"
        ))),
    }
}
