//! Construction of inert handle actions and standard-handle values.

use super::*;
use crate::handle::{Buffering, FileMode, STDERR_HANDLE, STDIN_HANDLE, STDOUT_HANDLE};

pub(super) fn open_file(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_value(args, 0, "openFile")?;
    let mode = decode_file_mode(ctx, expect_value(args, 1, "openFile")?)?;
    Ok(ctx.alloc_io_action(IoAction::OpenFile { path, mode }))
}

pub(super) fn h_close(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = expect_handle(args, 0, "hClose")?;
    Ok(ctx.alloc_io_action(IoAction::CloseHandle { handle }))
}

pub(super) fn h_flush(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = expect_handle(args, 0, "hFlush")?;
    Ok(ctx.alloc_io_action(IoAction::FlushHandle { handle }))
}

pub(super) fn h_get_line(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = expect_handle(args, 0, "hGetLine")?;
    Ok(ctx.alloc_io_action(IoAction::GetHandleLine { handle }))
}

pub(super) fn h_get_contents(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let handle = expect_handle(args, 0, "hGetContents")?;
    Ok(ctx.alloc_io_action(IoAction::GetHandleContents { handle }))
}

pub(super) fn h_get_bytes(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = expect_handle(args, 0, "hGetBytes")?;
    let maximum = expect_int(args, 1, "hGetBytes")?;
    Ok(ctx.alloc_io_action(IoAction::GetHandleBytes { handle, maximum }))
}

pub(super) fn h_put_str(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    put_action(ctx, args, false, "hPutStr")
}

pub(super) fn h_put_str_ln(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    put_action(ctx, args, true, "hPutStrLn")
}

pub(super) fn h_put_bytes(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = expect_handle(args, 0, "hPutBytes")?;
    let bytes = expect_value(args, 1, "hPutBytes")?;
    Ok(ctx.alloc_io_action(IoAction::PutHandleBytes { handle, bytes }))
}

pub(super) fn h_is_eof(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = expect_handle(args, 0, "hIsEOF")?;
    Ok(ctx.alloc_io_action(IoAction::IsHandleEof { handle }))
}

pub(super) fn h_set_buffering(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let handle = expect_handle(args, 0, "hSetBuffering")?;
    let buffering = decode_buffering(ctx, expect_value(args, 1, "hSetBuffering")?)?;
    Ok(ctx.alloc_io_action(IoAction::SetHandleBuffering { handle, buffering }))
}

pub(super) fn stdin(_ctx: &mut VmCtx<'_, '_>, _args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::HandleId(STDIN_HANDLE.0))
}

pub(super) fn stdout(_ctx: &mut VmCtx<'_, '_>, _args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::HandleId(STDOUT_HANDLE.0))
}

pub(super) fn stderr(_ctx: &mut VmCtx<'_, '_>, _args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::HandleId(STDERR_HANDLE.0))
}

fn put_action(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
    newline: bool,
    builtin: &str,
) -> Result<Value, RuntimeError> {
    let handle = expect_handle(args, 0, builtin)?;
    let text = expect_value(args, 1, builtin)?;
    Ok(ctx.alloc_io_action(IoAction::PutHandle {
        handle,
        text,
        newline,
    }))
}

fn expect_handle(args: &[Value], index: usize, builtin: &str) -> Result<Value, RuntimeError> {
    let value = expect_value(args, index, builtin)?;
    match value {
        Value::HandleId(_) => Ok(value),
        _ => Err(RuntimeError::builtin(format!(
            "{builtin} expected Handle argument at position {}",
            index + 1
        ))),
    }
}

fn decode_file_mode(ctx: &VmCtx<'_, '_>, value: Value) -> Result<FileMode, RuntimeError> {
    Ok(match value_ctor(ctx, value, "openFile")?.0 {
        CtorRuntimeRole::PreludeIoModeRead => FileMode::Read,
        CtorRuntimeRole::PreludeIoModeWrite => FileMode::Write,
        CtorRuntimeRole::PreludeIoModeAppend => FileMode::Append,
        CtorRuntimeRole::PreludeIoModeReadWrite => FileMode::ReadWrite,
        _ => {
            return Err(RuntimeError::builtin(
                "openFile expected an IOMode".to_string(),
            ));
        }
    })
}

fn decode_buffering(ctx: &VmCtx<'_, '_>, value: Value) -> Result<Buffering, RuntimeError> {
    let (role, field) = value_ctor(ctx, value, "hSetBuffering")?;
    match role {
        CtorRuntimeRole::PreludeBufferModeNone => Ok(Buffering::None),
        CtorRuntimeRole::PreludeBufferModeLine => Ok(Buffering::Line),
        CtorRuntimeRole::PreludeBufferModeBlock => {
            let Some(size) = field else {
                return Err(RuntimeError::builtin(
                    "BlockBuffering runtime value had no size field".to_string(),
                ));
            };
            Ok(Buffering::Block(decode_optional_size(ctx, size)?))
        }
        _ => Err(RuntimeError::builtin(
            "hSetBuffering expected a BufferMode".to_string(),
        )),
    }
}

fn decode_optional_size(ctx: &VmCtx<'_, '_>, value: Value) -> Result<Option<i64>, RuntimeError> {
    let (role, field) = value_ctor(ctx, value, "hSetBuffering")?;
    match (role, field) {
        (CtorRuntimeRole::PreludeMaybeNothing, None) => Ok(None),
        (CtorRuntimeRole::PreludeMaybeJust, Some(Value::Int(size))) => Ok(Some(size)),
        _ => Err(RuntimeError::builtin(
            "BlockBuffering expected Maybe Int".to_string(),
        )),
    }
}

fn value_ctor(
    ctx: &VmCtx<'_, '_>,
    value: Value,
    builtin: &str,
) -> Result<(CtorRuntimeRole, Option<Value>), RuntimeError> {
    let Value::Obj(id) = value else {
        return Err(RuntimeError::builtin(format!(
            "{builtin} expected a data constructor"
        )));
    };
    let (tag, field) = match ctx.interpreter.heap.get(id) {
        HeapObject::Data0 { tag } => (*tag, None),
        HeapObject::Data1 { tag, field } => (*tag, Some(*field)),
        _ => {
            return Err(RuntimeError::builtin(format!(
                "{builtin} expected a nullary or unary data constructor"
            )));
        }
    };
    let role = ctx
        .interpreter
        .program
        .constructor_arities
        .iter()
        .find(|info| info.tag == tag)
        .map(|info| info.role)
        .ok_or_else(|| RuntimeError::builtin(format!("{builtin} constructor metadata missing")))?;
    Ok((role, field))
}
