//! IO outcome unwinding, typed failure construction, and operand decoding.

use super::io_worker::IoFailure;
use super::io_worker::filesystem::FilesystemFailure;
use super::*;

fn host_error_kind(error: &std::io::Error) -> CtorRuntimeRole {
    match error.kind() {
        std::io::ErrorKind::AlreadyExists => CtorRuntimeRole::PreludeHostAlreadyExists,
        std::io::ErrorKind::NotFound => CtorRuntimeRole::PreludeHostDoesNotExist,
        std::io::ErrorKind::PermissionDenied => CtorRuntimeRole::PreludeHostPermissionDenied,
        std::io::ErrorKind::WouldBlock => CtorRuntimeRole::PreludeHostResourceBusy,
        std::io::ErrorKind::OutOfMemory | std::io::ErrorKind::WriteZero => {
            CtorRuntimeRole::PreludeHostResourceExhausted
        }
        std::io::ErrorKind::UnexpectedEof => CtorRuntimeRole::PreludeHostEndOfFile,
        std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => {
            CtorRuntimeRole::PreludeHostInvalidArgument
        }
        std::io::ErrorKind::Unsupported => CtorRuntimeRole::PreludeHostUnsupportedOperation,
        std::io::ErrorKind::TimedOut => CtorRuntimeRole::PreludeHostTimeExpired,
        std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted
        | std::io::ErrorKind::NotConnected
        | std::io::ErrorKind::BrokenPipe => CtorRuntimeRole::PreludeHostResourceVanished,
        std::io::ErrorKind::Interrupted => CtorRuntimeRole::PreludeHostInterrupted,
        std::io::ErrorKind::NetworkUnreachable | std::io::ErrorKind::AddrNotAvailable => {
            CtorRuntimeRole::PreludeHostNetworkUnreachable
        }
        std::io::ErrorKind::ConnectionRefused => CtorRuntimeRole::PreludeHostConnectionRefused,
        std::io::ErrorKind::AddrInUse => CtorRuntimeRole::PreludeHostAddressInUse,
        _ => CtorRuntimeRole::PreludeHostOtherError,
    }
}

use crate::builtins::VmCtx;
use crate::bytecode::BytecodeProgram;
use crate::handle::{HandleFailure, HandleId};
use crate::heap::{HeapObject, IoAction};
use crate::values::{Continuation, Value};
use solveig_ast::ast::CtorRuntimeRole;

impl Interpreter<'_> {
    pub(super) fn resume_io_defect(
        &mut self,
        program: &BytecodeProgram,
        error: RuntimeError,
    ) -> Result<(), StepOutcome> {
        let Some(kind) = error.defect_kind().cloned() else {
            return Err(StepOutcome::Failed(error));
        };
        let recover_heap = matches!(kind, DefectKind::HeapExhausted);
        let defect = self.alloc_defect(kind)?;
        self.resume_io_defect_value(program, defect, recover_heap, Some(error))
    }

    pub(super) fn resume_io_defect_value(
        &mut self,
        program: &BytecodeProgram,
        defect: Value,
        recover_heap: bool,
        mut boundary_error: Option<RuntimeError>,
    ) -> Result<(), StepOutcome> {
        loop {
            let frame_base = self.frame_base;
            self.stack.truncate(frame_base);
            let Some(frame) = self.continuations.pop() else {
                let error = boundary_error.unwrap_or_else(|| self.render_uncaught_defect(defect));
                return Err(StepOutcome::Failed(error));
            };

            match frame {
                Continuation::CallerFrame {
                    chunk,
                    pc,
                    frame_base,
                } => {
                    self.chunk = chunk;
                    self.pc = pc;
                    self.frame_base = frame_base;
                }
                Continuation::PublishGlobalThenResume {
                    global_index,
                    chunk,
                    pc,
                    frame_base,
                } => {
                    let error = boundary_error
                        .get_or_insert_with(|| self.render_uncaught_defect(defect))
                        .clone();
                    self.publish_global_defect(global_index, &error);
                    self.chunk = chunk;
                    self.pc = pc;
                    self.frame_base = frame_base;
                }
                Continuation::PublishGlobalThenHalt { global_index } => {
                    let error = boundary_error
                        .get_or_insert_with(|| self.render_uncaught_defect(defect))
                        .clone();
                    self.publish_global_defect(global_index, &error);
                    self.frame_base = self.stack.len();
                }
                Continuation::Halt => {
                    self.frame_base = self.stack.len();
                }
                Continuation::IoBind(_)
                | Continuation::IoCatch(_)
                | Continuation::IoRestore(_)
                | Continuation::IoRestoreDefect(_) => {}
                Continuation::IoFinally(cleanup) => {
                    self.continuations
                        .push(Continuation::IoRestoreDefect(defect));
                    let cleanup = self.root_across_defect_recovery(cleanup, recover_heap);
                    self.io_current = Some(cleanup);
                    return Ok(());
                }
                Continuation::IoRestoreMask(mask_depth)
                | Continuation::IoRestoreInterruption { mask_depth, .. }
                | Continuation::IoRestoreExit { mask_depth, .. } => {
                    self.mask_depth = mask_depth;
                }
                Continuation::IoCatchDefect(handler) => {
                    let checkpoint = self.temporary_roots.len();
                    self.temporary_roots.extend([handler, defect]);
                    if recover_heap {
                        self.collect_for_defect_recovery();
                    }
                    let handler = *self
                        .temporary_roots
                        .get(checkpoint)
                        .expect("ICE: defect handler root disappeared during recovery");
                    let defect = *self
                        .temporary_roots
                        .get(checkpoint + 1)
                        .expect("ICE: defect value root disappeared during recovery");
                    self.temporary_roots.truncate(checkpoint);
                    self.io_current = None;
                    match self.call_value_for_result(program, handler, &[defect]) {
                        Ok(next) => self.io_current = Some(next),
                        Err(StepOutcome::Failed(error)) if error.defect_kind().is_some() => {
                            return self.resume_io_defect(program, error);
                        }
                        Err(outcome) => return Err(outcome),
                    }
                    return Ok(());
                }
            }
        }
    }

    pub(super) fn root_across_defect_recovery(
        &mut self,
        value: Value,
        recover_heap: bool,
    ) -> Value {
        if !recover_heap {
            return value;
        }
        let checkpoint = self.temporary_roots.len();
        self.temporary_roots.push(value);
        self.collect_for_defect_recovery();
        let value = *self
            .temporary_roots
            .get(checkpoint)
            .expect("ICE: defect recovery root disappeared during collection");
        self.temporary_roots.truncate(checkpoint);
        value
    }

    pub(super) fn defect_requires_heap_recovery(&self, defect: Value) -> bool {
        let Value::Obj(id) = defect else {
            return false;
        };
        let tag = match self.heap.get(id) {
            HeapObject::Data0 { tag }
            | HeapObject::Data1 { tag, .. }
            | HeapObject::Data2 { tag, .. }
            | HeapObject::DataN { tag, .. } => *tag,
            HeapObject::Closure { .. }
            | HeapObject::PartialApp { .. }
            | HeapObject::PartialCtor { .. }
            | HeapObject::Str { .. }
            | HeapObject::Bytes { .. }
            | HeapObject::Chan(_)
            | HeapObject::IoAction(_)
            | HeapObject::Forwarded(_) => return false,
        };
        self.program
            .constructor_arities
            .iter()
            .any(|info| info.tag == tag && info.role == CtorRuntimeRole::PreludeDefectHeapExhausted)
    }

    pub(super) fn alloc_defect(&mut self, defect: DefectKind) -> Result<Value, RuntimeError> {
        match defect {
            DefectKind::Error(message) => {
                self.alloc_string_ctor(CtorRuntimeRole::PreludeDefectError, &message, "error")
            }
            DefectKind::DivisionByZero => VmCtx::new(self)
                .nullary_ctor(CtorRuntimeRole::PreludeDefectDivisionByZero, "division"),
            DefectKind::StackExhausted => VmCtx::new(self).nullary_ctor(
                CtorRuntimeRole::PreludeDefectStackExhausted,
                "stack exhaustion",
            ),
            DefectKind::HeapExhausted => VmCtx::new(self).nullary_ctor(
                CtorRuntimeRole::PreludeDefectHeapExhausted,
                "heap exhaustion",
            ),
        }
    }

    pub(super) fn render_uncaught_defect(&self, defect: Value) -> RuntimeError {
        let rendered = DisplayValue {
            root: defect,
            heap: &*self.heap,
            arities: &self.program.constructor_arities,
        }
        .to_string();
        RuntimeError::restored_defect(&rendered)
    }

    pub(super) fn publish_global_defect(&mut self, global_index: usize, error: &RuntimeError) {
        let process_id = self.id;
        let Some(slot) = self.globals.get_mut(global_index) else {
            solveig_base::ice!("defect unwind reached an out-of-bounds global index");
        };
        let waiters = match slot {
            GlobalSlot::Evaluating { owner, waiters } if *owner == process_id => {
                std::mem::take(waiters)
            }
            GlobalSlot::Evaluating { .. }
            | GlobalSlot::Thunk { .. }
            | GlobalSlot::Evaluated(_)
            | GlobalSlot::Failed(_) => {
                solveig_base::ice!("defect unwind lost ownership of its global blackhole")
            }
        };
        *slot = GlobalSlot::Failed(error.clone());

        for waiter_id in waiters {
            let waiter = &mut self.inactive_processes[waiter_id];
            if waiter.status
                != ProcStatus::Blocked(BlockReason::Blackhole(GlobalIndex(global_index)))
            {
                solveig_base::ice!("defected global waiter has a mismatched process status");
            }
            waiter.pending_defect = Some(error.clone());
            waiter.status = ProcStatus::Runnable;
            self.run_queue.push_back(waiter_id);
        }
    }

    pub(super) fn alloc_io_success_action(&mut self, value: Value) -> Value {
        let root = self.temporary_roots.len();
        self.temporary_roots.push(value);
        self.safe_point();
        let value = *self
            .temporary_roots
            .get(root)
            .expect("ICE: IO success root disappeared during allocation");
        let action = self.heap.alloc(HeapObject::IoAction(IoAction::Pure(value)));
        self.temporary_roots.truncate(root);
        action
    }

    pub(super) fn alloc_io_failure_action(
        &mut self,
        failure: IoFailure,
    ) -> Result<Value, RuntimeError> {
        let error = self.alloc_host_error_for_failure(failure)?;
        self.alloc_typed_failure_action(error)
    }

    pub(super) fn alloc_channel_closed_action(&mut self) -> Result<Value, RuntimeError> {
        let error = self.alloc_host_error(
            CtorRuntimeRole::PreludeHostResourceVanished,
            "channel operation",
            "channel is closed",
        )?;
        self.alloc_typed_failure_action(error)
    }

    pub(super) fn alloc_typed_failure_action(
        &mut self,
        error: Value,
    ) -> Result<Value, RuntimeError> {
        let root = self.temporary_roots.len();
        self.temporary_roots.push(error);
        self.safe_point();
        let error = *self
            .temporary_roots
            .get(root)
            .expect("ICE: IO error root disappeared during allocation");
        let action = self.heap.alloc(HeapObject::IoAction(IoAction::Fail(error)));
        self.temporary_roots.truncate(root);
        Ok(action)
    }

    pub(super) fn alloc_maybe_value(
        &mut self,
        value: Option<Value>,
    ) -> Result<Value, RuntimeError> {
        let Some(value) = value else {
            return VmCtx::new(self).nullary_ctor(CtorRuntimeRole::PreludeMaybeNothing, "tryRecv");
        };

        let root = self.temporary_roots.len();
        self.temporary_roots.push(value);
        self.safe_point();
        let value = *self
            .temporary_roots
            .get(root)
            .expect("ICE: tryRecv value root disappeared during allocation");
        let result =
            VmCtx::new(self).alloc_data1_ctor(CtorRuntimeRole::PreludeMaybeJust, value, "tryRecv");
        self.temporary_roots.truncate(root);
        result
    }

    pub(super) fn alloc_host_error_for_failure(
        &mut self,
        failure: IoFailure,
    ) -> Result<Value, RuntimeError> {
        match failure {
            IoFailure::Print(error) => self.alloc_std_io_error("print", &error),
            IoFailure::ReadLine(error) => self.alloc_std_io_error("readLine", &error),
            IoFailure::ReadFile { path, source } => {
                self.alloc_path_io_error("readFile", &path, &source)
            }
            IoFailure::ReadFileLimited { path, source } => {
                self.alloc_path_io_error("readFileLimited", &path, &source)
            }
            IoFailure::ReadFileTooLarge { path, maximum } => self.alloc_host_error(
                CtorRuntimeRole::PreludeHostResourceExhausted,
                "readFileLimited",
                &format!(
                    "{} exceeds the configured limit of {maximum} bytes",
                    path.display()
                ),
            ),
            IoFailure::WriteFile {
                path,
                append,
                source,
            } => self.alloc_path_io_error(
                if append { "appendFile" } else { "writeFile" },
                &path,
                &source,
            ),
            IoFailure::GetEnv(message) => {
                self.alloc_host_error(CtorRuntimeRole::PreludeHostOtherError, "getEnv", &message)
            }
            IoFailure::SetEnv(message) => self.alloc_host_error(
                CtorRuntimeRole::PreludeHostInvalidArgument,
                "setEnv",
                &message,
            ),
            IoFailure::Process(failure) => self.alloc_host_error(
                host_error_kind(&failure.source),
                "runProcess",
                &format!("{}: {}", failure.program, failure.source),
            ),
            IoFailure::Filesystem(failure) => match failure {
                FilesystemFailure::GetCurrentDirectory(source) => {
                    self.alloc_std_io_error("getCurrentDirectory", &source)
                }
                FilesystemFailure::SetCurrentDirectory { path, source } => {
                    self.alloc_path_io_error("setCurrentDirectory", &path, &source)
                }
                FilesystemFailure::DoesFileExist { path, source } => {
                    self.alloc_path_io_error("doesFileExist", &path, &source)
                }
                FilesystemFailure::DoesDirectoryExist { path, source } => {
                    self.alloc_path_io_error("doesDirectoryExist", &path, &source)
                }
                FilesystemFailure::ListDirectory { path, source } => {
                    self.alloc_path_io_error("listDirectory", &path, &source)
                }
                FilesystemFailure::CreateDirectory { path, source } => {
                    self.alloc_path_io_error("createDirectory", &path, &source)
                }
                FilesystemFailure::RemoveFile { path, source } => {
                    self.alloc_path_io_error("removeFile", &path, &source)
                }
                FilesystemFailure::RenameFile {
                    source_path,
                    destination,
                    source,
                } => self.alloc_host_error(
                    host_error_kind(&source),
                    "renameFile",
                    &format!(
                        "{} -> {}: {source}",
                        source_path.display(),
                        destination.display()
                    ),
                ),
                FilesystemFailure::GetFileSize { path, source } => {
                    self.alloc_path_io_error("getFileSize", &path, &source)
                }
                FilesystemFailure::GetModificationTime { path, source } => {
                    self.alloc_path_io_error("getModificationTime", &path, &source)
                }
            },
            IoFailure::Handle(failure) => match failure {
                HandleFailure::Open { path, source } => {
                    self.alloc_path_io_error("openFile", &path, &source)
                }
                HandleFailure::Close { label, source } => {
                    self.alloc_labeled_io_error("hClose", &label, &source)
                }
                HandleFailure::Flush { label, source } => {
                    self.alloc_labeled_io_error("hFlush", &label, &source)
                }
                HandleFailure::Read { label, source } => {
                    self.alloc_labeled_io_error("handle read", &label, &source)
                }
                HandleFailure::Write { label, source } => {
                    self.alloc_labeled_io_error("handle write", &label, &source)
                }
                HandleFailure::SetBuffering { label, source } => {
                    self.alloc_labeled_io_error("hSetBuffering", &label, &source)
                }
            },
            IoFailure::Connect { host, port, source } => self.alloc_host_error(
                host_error_kind(&source),
                "connect",
                &format!("{host}:{port}: {source}"),
            ),
            IoFailure::ResolveHost { host, source } => self.alloc_host_error(
                host_error_kind(&source),
                "resolveHost",
                &format!("{host}: {source}"),
            ),
            IoFailure::Listen { port, source } => self.alloc_host_error(
                host_error_kind(&source),
                "listen",
                &format!("port {port}: {source}"),
            ),
            IoFailure::Accept(error) => self.alloc_std_io_error("accept", &error),
            IoFailure::Send(error) => self.alloc_std_io_error("sendBytes", &error),
            IoFailure::Recv(error) => self.alloc_std_io_error("recvBytes", &error),
            IoFailure::CloseSocket(error) => self.alloc_std_io_error("closeSocket", &error),
            IoFailure::Http(failure) => match failure {
                super::http::HttpFailure::Transport(message) => self.alloc_host_error(
                    CtorRuntimeRole::PreludeHostProtocolError,
                    "httpTransport",
                    &message,
                ),
                super::http::HttpFailure::Tls(message) => self.alloc_host_error(
                    CtorRuntimeRole::PreludeHostProtocolError,
                    "httpTls",
                    &message,
                ),
                super::http::HttpFailure::Timeout(message) => self.alloc_host_error(
                    CtorRuntimeRole::PreludeHostTimeExpired,
                    "httpTimeout",
                    &message,
                ),
                super::http::HttpFailure::Redirect(message) => self.alloc_host_error(
                    CtorRuntimeRole::PreludeHostProtocolError,
                    "httpRedirect",
                    &message,
                ),
                super::http::HttpFailure::HeadersTooLarge(maximum) => self.alloc_host_error(
                    CtorRuntimeRole::PreludeHostResourceExhausted,
                    "httpResponseHeaders",
                    &format!("response headers exceed {maximum} bytes"),
                ),
                super::http::HttpFailure::BodyTooLarge(maximum) => self.alloc_host_error(
                    CtorRuntimeRole::PreludeHostResourceExhausted,
                    "httpResponseBody",
                    &format!("response body exceeds {maximum} bytes"),
                ),
            },
        }
    }

    fn alloc_std_io_error(
        &mut self,
        operation: &str,
        error: &std::io::Error,
    ) -> Result<Value, RuntimeError> {
        self.alloc_host_error(host_error_kind(error), operation, &error.to_string())
    }

    fn alloc_path_io_error(
        &mut self,
        operation: &str,
        path: &std::path::Path,
        error: &std::io::Error,
    ) -> Result<Value, RuntimeError> {
        self.alloc_host_error(
            host_error_kind(error),
            operation,
            &format!("{}: {error}", path.display()),
        )
    }

    fn alloc_labeled_io_error(
        &mut self,
        operation: &str,
        label: &str,
        error: &std::io::Error,
    ) -> Result<Value, RuntimeError> {
        self.alloc_host_error(
            host_error_kind(error),
            operation,
            &format!("{label}: {error}"),
        )
    }

    fn alloc_host_error(
        &mut self,
        kind_role: CtorRuntimeRole,
        operation: &str,
        message: &str,
    ) -> Result<Value, RuntimeError> {
        let checkpoint = self.temporary_roots.len();
        let kind = VmCtx::new(self).nullary_ctor(kind_role, operation)?;
        self.temporary_roots.push(kind);
        self.safe_point();
        let operation_value = self.heap.alloc_str(operation);
        self.temporary_roots.push(operation_value);
        self.safe_point();
        let message_value = self.heap.alloc_str(message);
        self.temporary_roots.push(message_value);
        self.safe_point();
        let &[kind, operation_value, message_value] = self
            .temporary_roots
            .get(checkpoint..checkpoint + 3)
            .expect("ICE: HostError roots disappeared during allocation")
        else {
            solveig_base::ice!("HostError root slice did not retain exactly three values")
        };
        let result = VmCtx::new(self).alloc_data3_ctor(
            CtorRuntimeRole::PreludeHostError,
            kind,
            operation_value,
            message_value,
            operation,
        );
        self.temporary_roots.truncate(checkpoint);
        result
    }

    pub(super) fn alloc_string_ctor(
        &mut self,
        role: CtorRuntimeRole,
        message: &str,
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        self.safe_point();
        let root = self.temporary_roots.len();
        let message = self.heap.alloc_str(message);
        self.temporary_roots.push(message);
        self.safe_point();
        let message = *self
            .temporary_roots
            .get(root)
            .expect("ICE: IO error message root disappeared during allocation");
        let result = VmCtx::new(self).alloc_data1_ctor(role, message, builtin);
        self.temporary_roots.truncate(root);
        result
    }

    pub(super) fn expect_io_action(&self, value: Value) -> Result<IoAction, RuntimeError> {
        match value {
            Value::Obj(id) => match self.heap.get(id) {
                HeapObject::IoAction(action) => Ok(*action),
                _ => Err(RuntimeError::builtin(
                    "main returned a non-IO heap value while performing IO".to_string(),
                )),
            },
            _ => Err(RuntimeError::builtin(
                "main returned a non-IO value while performing IO".to_string(),
            )),
        }
    }

    pub(super) fn expect_string_value(
        &self,
        value: Value,
        context: &str,
    ) -> Result<String, RuntimeError> {
        match value {
            Value::Obj(id) => match self.heap.get(id) {
                HeapObject::Str { bytes } => Ok(self.heap.str_value(id, *bytes).to_string()),
                _ => Err(RuntimeError::builtin(format!("{context} expected String"))),
            },
            _ => Err(RuntimeError::builtin(format!("{context} expected String"))),
        }
    }

    pub(super) fn expect_bytes_value(
        &self,
        value: Value,
        context: &str,
    ) -> Result<Vec<u8>, RuntimeError> {
        match value {
            Value::Obj(id) => match self.heap.get(id) {
                HeapObject::Bytes { bytes } => Ok(self.heap.bytes_value(id, *bytes).to_vec()),
                _ => Err(RuntimeError::builtin(format!("{context} expected Bytes"))),
            },
            _ => Err(RuntimeError::builtin(format!("{context} expected Bytes"))),
        }
    }

    pub(super) fn expect_socket_id(
        &self,
        value: Value,
        context: &str,
    ) -> Result<SocketId, RuntimeError> {
        match value {
            Value::SocketId(id) => Ok(SocketId(id)),
            _ => Err(RuntimeError::builtin(format!("{context} expected Socket"))),
        }
    }

    pub(super) fn expect_handle_id(
        &self,
        value: Value,
        context: &str,
    ) -> Result<HandleId, RuntimeError> {
        match value {
            Value::HandleId(id) => Ok(HandleId(id)),
            _ => Err(RuntimeError::builtin(format!("{context} expected Handle"))),
        }
    }
}

#[cfg(test)]
mod host_error_kind_tests {
    use super::host_error_kind;
    use solveig_ast::ast::CtorRuntimeRole;
    use std::io::ErrorKind;

    #[test]
    fn classifies_portable_io_error_kinds() {
        let cases = [
            (
                ErrorKind::AlreadyExists,
                CtorRuntimeRole::PreludeHostAlreadyExists,
            ),
            (
                ErrorKind::NotFound,
                CtorRuntimeRole::PreludeHostDoesNotExist,
            ),
            (
                ErrorKind::PermissionDenied,
                CtorRuntimeRole::PreludeHostPermissionDenied,
            ),
            (
                ErrorKind::WouldBlock,
                CtorRuntimeRole::PreludeHostResourceBusy,
            ),
            (
                ErrorKind::OutOfMemory,
                CtorRuntimeRole::PreludeHostResourceExhausted,
            ),
            (
                ErrorKind::UnexpectedEof,
                CtorRuntimeRole::PreludeHostEndOfFile,
            ),
            (
                ErrorKind::InvalidInput,
                CtorRuntimeRole::PreludeHostInvalidArgument,
            ),
            (
                ErrorKind::Unsupported,
                CtorRuntimeRole::PreludeHostUnsupportedOperation,
            ),
            (ErrorKind::TimedOut, CtorRuntimeRole::PreludeHostTimeExpired),
            (
                ErrorKind::BrokenPipe,
                CtorRuntimeRole::PreludeHostResourceVanished,
            ),
            (
                ErrorKind::Interrupted,
                CtorRuntimeRole::PreludeHostInterrupted,
            ),
            (
                ErrorKind::NetworkUnreachable,
                CtorRuntimeRole::PreludeHostNetworkUnreachable,
            ),
            (
                ErrorKind::ConnectionRefused,
                CtorRuntimeRole::PreludeHostConnectionRefused,
            ),
            (
                ErrorKind::AddrInUse,
                CtorRuntimeRole::PreludeHostAddressInUse,
            ),
            (ErrorKind::Other, CtorRuntimeRole::PreludeHostOtherError),
        ];

        for (kind, expected) in cases {
            assert_eq!(host_error_kind(&std::io::Error::from(kind)), expected);
        }
    }
}
