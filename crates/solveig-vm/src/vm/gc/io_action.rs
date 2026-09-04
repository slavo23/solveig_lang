//! Exhaustive GC traversal for values retained by inert IO actions.

use super::*;
use crate::heap::{EnvironmentAction, FilesystemAction};

pub(super) fn rewrite(
    heap: &mut Heap,
    action: &mut IoAction,
    worklist: &mut Vec<ObjId>,
    promoted: &mut usize,
) {
    visit_action_mut(action, |value| {
        evacuate_value(heap, value, worklist, promoted);
    });
}

pub(super) fn mark_old(heap: &mut Heap, action: IoAction, worklist: &mut Vec<ObjId>) {
    visit_action(action, |value| mark_old_child(heap, value, worklist));
}

#[cfg(debug_assertions)]
pub(super) fn assert_live(
    heap: &Heap,
    action: IoAction,
    seen: &mut [bool],
    worklist: &mut Vec<ObjId>,
) {
    visit_action(action, |value| {
        assert_value_live(heap, value, seen, worklist);
    });
}

fn visit_action_mut(action: &mut IoAction, mut visit: impl FnMut(&mut Value)) {
    match action {
        IoAction::Pure(value)
        | IoAction::Fail(value)
        | IoAction::RaiseDefect { defect: value }
        | IoAction::Print { text: value, .. }
        | IoAction::Spawn { action: value }
        | IoAction::KillThread { thread: value }
        | IoAction::Mask { action: value }
        | IoAction::RestoreMask { action: value }
        | IoAction::ResolveHost { host: value }
        | IoAction::Connect { host: value, .. }
        | IoAction::Accept { socket: value }
        | IoAction::RecvBytes { socket: value, .. }
        | IoAction::RecvDatagram { socket: value, .. }
        | IoAction::CloseSocket { socket: value } => visit(value),
        IoAction::Bind {
            action,
            continuation,
        }
        | IoAction::Catch {
            action,
            handler: continuation,
        }
        | IoAction::CatchDefect {
            action,
            handler: continuation,
        }
        | IoAction::Finally {
            action,
            cleanup: continuation,
        } => {
            visit(action);
            visit(continuation);
        }
        IoAction::ReadFile { path, .. }
        | IoAction::ReadFileBytes { path, .. }
        | IoAction::OpenFile { path, .. } => visit(path),
        IoAction::WriteFile { path, contents, .. }
        | IoAction::WriteFileBytes { path, contents } => {
            visit(path);
            visit(contents);
        }
        IoAction::RunProcess { request } | IoAction::StartProcess { request } => visit(request),
        IoAction::Send { channel, value } => {
            visit(channel);
            visit(value);
        }
        IoAction::SendBytes { socket, bytes }
        | IoAction::StartTlsClient {
            socket,
            config: bytes,
        } => {
            visit(socket);
            visit(bytes);
        }
        IoAction::SendDatagram { socket, request } => {
            visit(socket);
            visit(request);
        }
        IoAction::WriteProcessStdin { process, bytes } => {
            visit(process);
            visit(bytes);
        }
        IoAction::HttpRequest { request, limits }
        | IoAction::HttpOpen { request, limits }
        | IoAction::HttpUploadStart { request, limits } => {
            visit(request);
            visit(limits);
        }
        IoAction::HttpUploadChunk { handle, bytes } => {
            visit(handle);
            visit(bytes);
        }
        IoAction::Recv { channel }
        | IoAction::TryRecv { channel }
        | IoAction::CloseChan { channel }
        | IoAction::HttpUploadClose { handle: channel }
        | IoAction::CloseHandle { handle: channel }
        | IoAction::FlushHandle { handle: channel }
        | IoAction::GetHandleLine { handle: channel }
        | IoAction::GetHandleContents { handle: channel }
        | IoAction::GetHandleBytes {
            handle: channel, ..
        }
        | IoAction::IsHandleEof { handle: channel }
        | IoAction::SetHandleBuffering {
            handle: channel, ..
        }
        | IoAction::ReadProcessStdout {
            process: channel, ..
        }
        | IoAction::ReadProcessStderr {
            process: channel, ..
        }
        | IoAction::CloseProcessStdin { process: channel }
        | IoAction::WaitProcess { process: channel }
        | IoAction::TerminateProcess { process: channel }
        | IoAction::CloseProcess { process: channel } => visit(channel),
        IoAction::PutHandle { handle, text, .. }
        | IoAction::PutHandleBytes {
            handle,
            bytes: text,
        } => {
            visit(handle);
            visit(text);
        }
        IoAction::Environment(action) => visit_environment_mut(action, visit),
        IoAction::Filesystem(action) => visit_filesystem_mut(action, visit),
        IoAction::ReadLine
        | IoAction::GetArgs
        | IoAction::NewChan { .. }
        | IoAction::Sleep { .. }
        | IoAction::GetMonotonicTime
        | IoAction::GetSystemTime
        | IoAction::NewStdGen
        | IoAction::SecureRandomBytes { .. }
        | IoAction::Listen { .. }
        | IoAction::BindUdp { .. }
        | IoAction::Exit { .. } => {}
    }
}

fn visit_action(action: IoAction, mut visit: impl FnMut(Value)) {
    match action {
        IoAction::Pure(value)
        | IoAction::Fail(value)
        | IoAction::RaiseDefect { defect: value }
        | IoAction::Print { text: value, .. }
        | IoAction::Spawn { action: value }
        | IoAction::KillThread { thread: value }
        | IoAction::Mask { action: value }
        | IoAction::RestoreMask { action: value }
        | IoAction::ResolveHost { host: value }
        | IoAction::Connect { host: value, .. }
        | IoAction::Accept { socket: value }
        | IoAction::RecvBytes { socket: value, .. }
        | IoAction::RecvDatagram { socket: value, .. }
        | IoAction::CloseSocket { socket: value } => visit(value),
        IoAction::Bind {
            action,
            continuation,
        }
        | IoAction::Catch {
            action,
            handler: continuation,
        }
        | IoAction::CatchDefect {
            action,
            handler: continuation,
        }
        | IoAction::Finally {
            action,
            cleanup: continuation,
        } => {
            visit(action);
            visit(continuation);
        }
        IoAction::ReadFile { path, .. }
        | IoAction::ReadFileBytes { path, .. }
        | IoAction::OpenFile { path, .. } => visit(path),
        IoAction::WriteFile { path, contents, .. }
        | IoAction::WriteFileBytes { path, contents } => {
            visit(path);
            visit(contents);
        }
        IoAction::RunProcess { request } | IoAction::StartProcess { request } => visit(request),
        IoAction::Send { channel, value } => {
            visit(channel);
            visit(value);
        }
        IoAction::SendBytes { socket, bytes }
        | IoAction::StartTlsClient {
            socket,
            config: bytes,
        } => {
            visit(socket);
            visit(bytes);
        }
        IoAction::SendDatagram { socket, request } => {
            visit(socket);
            visit(request);
        }
        IoAction::WriteProcessStdin { process, bytes } => {
            visit(process);
            visit(bytes);
        }
        IoAction::HttpRequest { request, limits }
        | IoAction::HttpOpen { request, limits }
        | IoAction::HttpUploadStart { request, limits } => {
            visit(request);
            visit(limits);
        }
        IoAction::HttpUploadChunk { handle, bytes } => {
            visit(handle);
            visit(bytes);
        }
        IoAction::Recv { channel }
        | IoAction::TryRecv { channel }
        | IoAction::CloseChan { channel }
        | IoAction::HttpUploadClose { handle: channel }
        | IoAction::CloseHandle { handle: channel }
        | IoAction::FlushHandle { handle: channel }
        | IoAction::GetHandleLine { handle: channel }
        | IoAction::GetHandleContents { handle: channel }
        | IoAction::GetHandleBytes {
            handle: channel, ..
        }
        | IoAction::IsHandleEof { handle: channel }
        | IoAction::SetHandleBuffering {
            handle: channel, ..
        }
        | IoAction::ReadProcessStdout {
            process: channel, ..
        }
        | IoAction::ReadProcessStderr {
            process: channel, ..
        }
        | IoAction::CloseProcessStdin { process: channel }
        | IoAction::WaitProcess { process: channel }
        | IoAction::TerminateProcess { process: channel }
        | IoAction::CloseProcess { process: channel } => visit(channel),
        IoAction::PutHandle { handle, text, .. }
        | IoAction::PutHandleBytes {
            handle,
            bytes: text,
        } => {
            visit(handle);
            visit(text);
        }
        IoAction::Environment(action) => visit_environment(action, visit),
        IoAction::Filesystem(action) => visit_filesystem(action, visit),
        IoAction::ReadLine
        | IoAction::GetArgs
        | IoAction::NewChan { .. }
        | IoAction::Sleep { .. }
        | IoAction::GetMonotonicTime
        | IoAction::GetSystemTime
        | IoAction::NewStdGen
        | IoAction::SecureRandomBytes { .. }
        | IoAction::Listen { .. }
        | IoAction::BindUdp { .. }
        | IoAction::Exit { .. } => {}
    }
}

fn visit_environment_mut(action: &mut EnvironmentAction, mut visit: impl FnMut(&mut Value)) {
    match action {
        EnvironmentAction::Get { name } => visit(name),
        EnvironmentAction::Set { name, value } => {
            visit(name);
            visit(value);
        }
        EnvironmentAction::GetAll | EnvironmentAction::GetProgramName => {}
    }
}

fn visit_environment(action: EnvironmentAction, mut visit: impl FnMut(Value)) {
    match action {
        EnvironmentAction::Get { name } => visit(name),
        EnvironmentAction::Set { name, value } => {
            visit(name);
            visit(value);
        }
        EnvironmentAction::GetAll | EnvironmentAction::GetProgramName => {}
    }
}

fn visit_filesystem_mut(action: &mut FilesystemAction, mut visit: impl FnMut(&mut Value)) {
    match action {
        FilesystemAction::GetCurrentDirectory => {}
        FilesystemAction::SetCurrentDirectory { path }
        | FilesystemAction::DoesFileExist { path }
        | FilesystemAction::DoesDirectoryExist { path }
        | FilesystemAction::ListDirectory { path }
        | FilesystemAction::CreateDirectory { path }
        | FilesystemAction::RemoveFile { path }
        | FilesystemAction::GetFileSize { path }
        | FilesystemAction::GetModificationTime { path } => visit(path),
        FilesystemAction::RenameFile {
            source,
            destination,
        } => {
            visit(source);
            visit(destination);
        }
        FilesystemAction::WriteFileAtomic { path, contents, .. } => {
            visit(path);
            visit(contents);
        }
    }
}

fn visit_filesystem(action: FilesystemAction, mut visit: impl FnMut(Value)) {
    match action {
        FilesystemAction::GetCurrentDirectory => {}
        FilesystemAction::SetCurrentDirectory { path }
        | FilesystemAction::DoesFileExist { path }
        | FilesystemAction::DoesDirectoryExist { path }
        | FilesystemAction::ListDirectory { path }
        | FilesystemAction::CreateDirectory { path }
        | FilesystemAction::RemoveFile { path }
        | FilesystemAction::GetFileSize { path }
        | FilesystemAction::GetModificationTime { path } => visit(path),
        FilesystemAction::RenameFile {
            source,
            destination,
        } => {
            visit(source);
            visit(destination);
        }
        FilesystemAction::WriteFileAtomic { path, contents, .. } => {
            visit(path);
            visit(contents);
        }
    }
}
