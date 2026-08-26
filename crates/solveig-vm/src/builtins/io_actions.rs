//! Construction of the core inert IO, concurrency, and whole-file actions.

use super::*;

pub(super) fn pure_io(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_value(args, 0, "pureIO")?;
    Ok(ctx.alloc_io_action(IoAction::Pure(value)))
}

pub(super) fn bind_io(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let action = expect_value(args, 0, "bindIO")?;
    let continuation = expect_value(args, 1, "bindIO")?;
    Ok(ctx.alloc_io_action(IoAction::Bind {
        action,
        continuation,
    }))
}

pub(super) fn fail_io(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let error = expect_value(args, 0, "failIO")?;
    Ok(ctx.alloc_io_action(IoAction::Fail(error)))
}

pub(super) fn raise_error(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let message = expect_string_owned(ctx, args, 0, "error")?;
    Err(RuntimeError::defect(DefectKind::Error(message)))
}

pub(super) fn catch_io(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let action = expect_value(args, 0, "catchIO")?;
    let handler = expect_value(args, 1, "catchIO")?;
    Ok(ctx.alloc_io_action(IoAction::Catch { action, handler }))
}

pub(super) fn catch_defect_io(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let action = expect_value(args, 0, "catchDefectIO")?;
    let handler = expect_value(args, 1, "catchDefectIO")?;
    Ok(ctx.alloc_io_action(IoAction::CatchDefect { action, handler }))
}

pub(super) fn raise_defect_io(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let defect = expect_value(args, 0, "raiseDefectIO")?;
    Ok(ctx.alloc_io_action(IoAction::RaiseDefect { defect }))
}

pub(super) fn finally_io(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let action = expect_value(args, 0, "finallyIO")?;
    let cleanup = expect_value(args, 1, "finallyIO")?;
    Ok(ctx.alloc_io_action(IoAction::Finally { action, cleanup }))
}

pub(super) fn print_line(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    print_action(ctx, args, true, "printLine")
}

pub(super) fn print_str(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    print_action(ctx, args, false, "printStr")
}

pub(super) fn read_line(ctx: &mut VmCtx<'_, '_>, _args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_io_action(IoAction::ReadLine))
}

pub(super) fn get_args(ctx: &mut VmCtx<'_, '_>, _args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_io_action(IoAction::GetArgs))
}

pub(super) fn read_file(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_value(args, 0, "readFile")?;
    Ok(ctx.alloc_io_action(IoAction::ReadFile {
        path,
        maximum: None,
    }))
}

pub(super) fn read_file_limited(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let path = expect_value(args, 0, "readFileLimited")?;
    let maximum = expect_int(args, 1, "readFileLimited")?;
    Ok(ctx.alloc_io_action(IoAction::ReadFile {
        path,
        maximum: Some(maximum),
    }))
}

pub(super) fn read_file_bytes(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let path = expect_value(args, 0, "readFileBytes")?;
    Ok(ctx.alloc_io_action(IoAction::ReadFileBytes {
        path,
        maximum: None,
    }))
}

pub(super) fn read_file_bytes_limited(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let path = expect_value(args, 0, "readFileBytesLimited")?;
    let maximum = expect_int(args, 1, "readFileBytesLimited")?;
    Ok(ctx.alloc_io_action(IoAction::ReadFileBytes {
        path,
        maximum: Some(maximum),
    }))
}

pub(super) fn write_file(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    write_file_action(ctx, args, false, "writeFile")
}

pub(super) fn write_file_bytes(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let path = expect_value(args, 0, "writeFileBytes")?;
    let contents = expect_value(args, 1, "writeFileBytes")?;
    Ok(ctx.alloc_io_action(IoAction::WriteFileBytes { path, contents }))
}

pub(super) fn write_file_atomic(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let path = expect_value(args, 0, "writeFileAtomic")?;
    let contents = expect_value(args, 1, "writeFileAtomic")?;
    Ok(ctx.alloc_io_action(IoAction::Filesystem(
        crate::heap::FilesystemAction::WriteFileAtomic {
            path,
            contents,
            bytes: false,
        },
    )))
}

pub(super) fn write_file_bytes_atomic(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let path = expect_value(args, 0, "writeFileBytesAtomic")?;
    let contents = expect_value(args, 1, "writeFileBytesAtomic")?;
    Ok(ctx.alloc_io_action(IoAction::Filesystem(
        crate::heap::FilesystemAction::WriteFileAtomic {
            path,
            contents,
            bytes: true,
        },
    )))
}

pub(super) fn append_file(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    write_file_action(ctx, args, true, "appendFile")
}

pub(super) fn spawn(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let action = expect_value(args, 0, "spawn")?;
    Ok(ctx.alloc_io_action(IoAction::Spawn { action }))
}

pub(super) fn new_chan(ctx: &mut VmCtx<'_, '_>, _args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_io_action(IoAction::NewChan { capacity: None }))
}

pub(super) fn new_bounded_chan(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let capacity = expect_int(args, 0, "newBoundedChan")?;
    Ok(ctx.alloc_io_action(IoAction::NewChan {
        capacity: Some(capacity),
    }))
}

pub(super) fn send_chan(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let channel = expect_value(args, 0, "send")?;
    let value = expect_value(args, 1, "send")?;
    Ok(ctx.alloc_io_action(IoAction::Send { channel, value }))
}

pub(super) fn recv_chan(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let channel = expect_value(args, 0, "recv")?;
    Ok(ctx.alloc_io_action(IoAction::Recv { channel }))
}

pub(super) fn try_recv_chan(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let channel = expect_value(args, 0, "tryRecv")?;
    Ok(ctx.alloc_io_action(IoAction::TryRecv { channel }))
}

pub(super) fn close_chan(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let channel = expect_value(args, 0, "closeChan")?;
    Ok(ctx.alloc_io_action(IoAction::CloseChan { channel }))
}

pub(super) fn kill_thread(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let thread = expect_value(args, 0, "killThread")?;
    Ok(ctx.alloc_io_action(IoAction::KillThread { thread }))
}

pub(super) fn mask_io(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let action = expect_value(args, 0, "maskIO")?;
    Ok(ctx.alloc_io_action(IoAction::Mask { action }))
}

pub(super) fn restore_mask_io(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let action = expect_value(args, 0, "restoreMaskIO")?;
    Ok(ctx.alloc_io_action(IoAction::RestoreMask { action }))
}

pub(super) fn sleep(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let milliseconds = expect_int(args, 0, "sleep")?;
    Ok(ctx.alloc_io_action(IoAction::Sleep { milliseconds }))
}

pub(super) fn get_monotonic_time(
    ctx: &mut VmCtx<'_, '_>,
    _args: &[Value],
) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_io_action(IoAction::GetMonotonicTime))
}

pub(super) fn get_system_time(
    ctx: &mut VmCtx<'_, '_>,
    _args: &[Value],
) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_io_action(IoAction::GetSystemTime))
}

fn print_action(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
    newline: bool,
    builtin: &str,
) -> Result<Value, RuntimeError> {
    let text = expect_value(args, 0, builtin)?;
    Ok(ctx.alloc_io_action(IoAction::Print { text, newline }))
}

fn write_file_action(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
    append: bool,
    builtin: &str,
) -> Result<Value, RuntimeError> {
    let path = expect_value(args, 0, builtin)?;
    let contents = expect_value(args, 1, builtin)?;
    Ok(ctx.alloc_io_action(IoAction::WriteFile {
        path,
        contents,
        append,
    }))
}
