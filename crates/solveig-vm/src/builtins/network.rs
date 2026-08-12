//! Network primitive constructors.
//! Host socket ownership and readiness execution remain in the VM IO subsystem.

use super::{VmCtx, expect_int, expect_value};
use crate::heap::IoAction;
use crate::values::Value;
use crate::vm::RuntimeError;

pub(super) fn connect(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let host = expect_value(args, 0, "connect")?;
    let port = expect_int(args, 1, "connect")?;
    Ok(ctx.alloc_io_action(IoAction::Connect { host, port }))
}

pub(super) fn listen(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let port = expect_int(args, 0, "listen")?;
    Ok(ctx.alloc_io_action(IoAction::Listen { port }))
}

pub(super) fn accept(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let socket = expect_value(args, 0, "accept")?;
    Ok(ctx.alloc_io_action(IoAction::Accept { socket }))
}

pub(super) fn send_bytes(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let socket = expect_value(args, 0, "sendBytes")?;
    let bytes = expect_value(args, 1, "sendBytes")?;
    Ok(ctx.alloc_io_action(IoAction::SendBytes { socket, bytes }))
}

pub(super) fn recv_bytes(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let socket = expect_value(args, 0, "recvBytes")?;
    let maximum = expect_int(args, 1, "recvBytes")?;
    Ok(ctx.alloc_io_action(IoAction::RecvBytes { socket, maximum }))
}

pub(super) fn close_socket(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let socket = expect_value(args, 0, "closeSocket")?;
    Ok(ctx.alloc_io_action(IoAction::CloseSocket { socket }))
}
