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

pub(super) fn resolve_host(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let host = expect_value(args, 0, "resolveHost")?;
    Ok(ctx.alloc_io_action(IoAction::ResolveHost { host }))
}

pub(super) fn http_request_raw(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let request = expect_value(args, 0, "httpRequestRaw")?;
    let limits = expect_value(args, 1, "httpRequestRaw")?;
    Ok(ctx.alloc_io_action(IoAction::HttpRequest { request, limits }))
}

pub(super) fn listen(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let port = expect_int(args, 0, "listen")?;
    Ok(ctx.alloc_io_action(IoAction::Listen { port }))
}

pub(super) fn http_open_raw(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let request = expect_value(args, 0, "httpOpenRaw")?;
    let limits = expect_value(args, 1, "httpOpenRaw")?;
    Ok(ctx.alloc_io_action(IoAction::HttpOpen { request, limits }))
}

pub(super) fn http_upload_start_raw(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let request = expect_value(args, 0, "httpUploadStartRaw")?;
    let limits = expect_value(args, 1, "httpUploadStartRaw")?;
    Ok(ctx.alloc_io_action(IoAction::HttpUploadStart { request, limits }))
}

pub(super) fn http_upload_chunk_raw(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let handle = expect_value(args, 0, "httpUploadChunkRaw")?;
    let bytes = expect_value(args, 1, "httpUploadChunkRaw")?;
    Ok(ctx.alloc_io_action(IoAction::HttpUploadChunk { handle, bytes }))
}

pub(super) fn http_upload_close_raw(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let handle = expect_value(args, 0, "httpUploadCloseRaw")?;
    Ok(ctx.alloc_io_action(IoAction::HttpUploadClose { handle }))
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

pub(super) fn bind_udp(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let port = expect_int(args, 0, "bindUdp")?;
    Ok(ctx.alloc_io_action(IoAction::BindUdp { port }))
}

pub(super) fn send_datagram(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let socket = expect_value(args, 0, "sendDatagram")?;
    let request = expect_value(args, 1, "sendDatagram")?;
    Ok(ctx.alloc_io_action(IoAction::SendDatagram { socket, request }))
}

pub(super) fn recv_datagram(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let socket = expect_value(args, 0, "recvDatagram")?;
    let maximum = expect_int(args, 1, "recvDatagram")?;
    Ok(ctx.alloc_io_action(IoAction::RecvDatagram { socket, maximum }))
}
