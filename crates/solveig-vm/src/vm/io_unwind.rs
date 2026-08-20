//! IO outcome unwinding, typed failure construction, and operand decoding.

use super::io_worker::IoFailure;
use super::io_worker::filesystem::FilesystemFailure;
use super::*;
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
        let error = self.alloc_io_error(failure)?;
        self.alloc_typed_failure_action(error)
    }

    pub(super) fn alloc_channel_closed_action(&mut self) -> Result<Value, RuntimeError> {
        let error = VmCtx::new(self).nullary_ctor(
            CtorRuntimeRole::PreludeIoErrorChannelClosed,
            "channel operation",
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

    pub(super) fn alloc_io_error(&mut self, failure: IoFailure) -> Result<Value, RuntimeError> {
        match failure {
            IoFailure::Print(error) => self.alloc_string_ctor(
                CtorRuntimeRole::PreludeIoErrorStdout,
                &error.to_string(),
                "print",
            ),
            IoFailure::ReadLine(error) => self.alloc_string_ctor(
                CtorRuntimeRole::PreludeIoErrorStdin,
                &error.to_string(),
                "readLine",
            ),
            IoFailure::ReadFile { path, source } => self.alloc_io_error2(
                CtorRuntimeRole::PreludeIoErrorReadFile,
                &path.display().to_string(),
                &source.to_string(),
                "readFile",
            ),
            IoFailure::ReadFileTooLarge { path, maximum } => self.alloc_io_error2(
                CtorRuntimeRole::PreludeIoErrorReadFile,
                &path.display().to_string(),
                &format!("file exceeds the configured limit of {maximum} bytes"),
                "readFileLimited",
            ),
            IoFailure::WriteFile {
                path,
                append,
                source,
            } => {
                let (role, builtin) = if append {
                    (CtorRuntimeRole::PreludeIoErrorAppendFile, "appendFile")
                } else {
                    (CtorRuntimeRole::PreludeIoErrorWriteFile, "writeFile")
                };
                self.alloc_io_error2(
                    role,
                    &path.display().to_string(),
                    &source.to_string(),
                    builtin,
                )
            }
            IoFailure::GetEnv(message) => {
                self.alloc_string_ctor(CtorRuntimeRole::PreludeIoErrorGetEnv, &message, "getEnv")
            }
            IoFailure::SetEnv(message) => {
                self.alloc_string_ctor(CtorRuntimeRole::PreludeIoErrorSetEnv, &message, "setEnv")
            }
            IoFailure::Process(failure) => self.alloc_io_error2(
                CtorRuntimeRole::PreludeIoErrorRunProcess,
                &failure.program,
                &failure.source.to_string(),
                "runProcess",
            ),
            IoFailure::Filesystem(failure) => match failure {
                FilesystemFailure::GetCurrentDirectory(source) => self.alloc_string_ctor(
                    CtorRuntimeRole::PreludeIoErrorGetCurrentDirectory,
                    &source.to_string(),
                    "getCurrentDirectory",
                ),
                FilesystemFailure::SetCurrentDirectory { path, source } => self.alloc_io_error2(
                    CtorRuntimeRole::PreludeIoErrorSetCurrentDirectory,
                    &path.display().to_string(),
                    &source.to_string(),
                    "setCurrentDirectory",
                ),
                FilesystemFailure::DoesFileExist { path, source } => self.alloc_io_error2(
                    CtorRuntimeRole::PreludeIoErrorDoesFileExist,
                    &path.display().to_string(),
                    &source.to_string(),
                    "doesFileExist",
                ),
                FilesystemFailure::DoesDirectoryExist { path, source } => self.alloc_io_error2(
                    CtorRuntimeRole::PreludeIoErrorDoesDirectoryExist,
                    &path.display().to_string(),
                    &source.to_string(),
                    "doesDirectoryExist",
                ),
                FilesystemFailure::ListDirectory { path, source } => self.alloc_io_error2(
                    CtorRuntimeRole::PreludeIoErrorListDirectory,
                    &path.display().to_string(),
                    &source.to_string(),
                    "listDirectory",
                ),
                FilesystemFailure::CreateDirectory { path, source } => self.alloc_io_error2(
                    CtorRuntimeRole::PreludeIoErrorCreateDirectory,
                    &path.display().to_string(),
                    &source.to_string(),
                    "createDirectory",
                ),
                FilesystemFailure::RemoveFile { path, source } => self.alloc_io_error2(
                    CtorRuntimeRole::PreludeIoErrorRemoveFile,
                    &path.display().to_string(),
                    &source.to_string(),
                    "removeFile",
                ),
                FilesystemFailure::RenameFile {
                    source_path,
                    destination,
                    source,
                } => self.alloc_io_error3(
                    CtorRuntimeRole::PreludeIoErrorRenameFile,
                    &source_path.display().to_string(),
                    &destination.display().to_string(),
                    &source.to_string(),
                    "renameFile",
                ),
                FilesystemFailure::GetFileSize { path, source } => self.alloc_io_error2(
                    CtorRuntimeRole::PreludeIoErrorGetFileSize,
                    &path.display().to_string(),
                    &source.to_string(),
                    "getFileSize",
                ),
                FilesystemFailure::GetModificationTime { path, source } => self.alloc_io_error2(
                    CtorRuntimeRole::PreludeIoErrorGetModificationTime,
                    &path.display().to_string(),
                    &source.to_string(),
                    "getModificationTime",
                ),
            },
            IoFailure::Handle(failure) => match failure {
                HandleFailure::Open { path, source } => self.alloc_io_error2(
                    CtorRuntimeRole::PreludeIoErrorOpenHandle,
                    &path.display().to_string(),
                    &source.to_string(),
                    "openFile",
                ),
                HandleFailure::Close { label, source } => self.alloc_string_ctor(
                    CtorRuntimeRole::PreludeIoErrorCloseHandle,
                    &format!("{label}: {source}"),
                    "hClose",
                ),
                HandleFailure::Flush { label, source } => self.alloc_string_ctor(
                    CtorRuntimeRole::PreludeIoErrorFlushHandle,
                    &format!("{label}: {source}"),
                    "hFlush",
                ),
                HandleFailure::Read { label, source } => self.alloc_string_ctor(
                    CtorRuntimeRole::PreludeIoErrorReadHandle,
                    &format!("{label}: {source}"),
                    "handle read",
                ),
                HandleFailure::Write { label, source } => self.alloc_string_ctor(
                    CtorRuntimeRole::PreludeIoErrorWriteHandle,
                    &format!("{label}: {source}"),
                    "handle write",
                ),
                HandleFailure::SetBuffering { label, source } => self.alloc_string_ctor(
                    CtorRuntimeRole::PreludeIoErrorSetBuffering,
                    &format!("{label}: {source}"),
                    "hSetBuffering",
                ),
            },
            IoFailure::Connect { host, port, source } => self.alloc_string_ctor(
                CtorRuntimeRole::PreludeIoErrorConnect,
                &format!("{host}:{port}: {source}"),
                "connect",
            ),
            IoFailure::ResolveHost { host, source } => self.alloc_string_ctor(
                CtorRuntimeRole::PreludeIoErrorResolveHost,
                &format!("{host}: {source}"),
                "resolveHost",
            ),
            IoFailure::Listen { port, source } => self.alloc_string_ctor(
                CtorRuntimeRole::PreludeIoErrorListen,
                &format!("port {port}: {source}"),
                "listen",
            ),
            IoFailure::Accept(error) => self.alloc_string_ctor(
                CtorRuntimeRole::PreludeIoErrorAccept,
                &error.to_string(),
                "accept",
            ),
            IoFailure::Send(error) => self.alloc_string_ctor(
                CtorRuntimeRole::PreludeIoErrorSend,
                &error.to_string(),
                "sendBytes",
            ),
            IoFailure::Recv(error) => self.alloc_string_ctor(
                CtorRuntimeRole::PreludeIoErrorRecv,
                &error.to_string(),
                "recvBytes",
            ),
            IoFailure::CloseSocket(error) => self.alloc_string_ctor(
                CtorRuntimeRole::PreludeIoErrorCloseSocket,
                &error.to_string(),
                "closeSocket",
            ),
            IoFailure::Http(failure) => match failure {
                super::http::HttpFailure::Transport(message) => self.alloc_string_ctor(
                    CtorRuntimeRole::PreludeIoErrorHttpTransport,
                    &message,
                    "httpRequestRaw",
                ),
                super::http::HttpFailure::Tls(message) => self.alloc_string_ctor(
                    CtorRuntimeRole::PreludeIoErrorHttpTls,
                    &message,
                    "httpRequestRaw",
                ),
                super::http::HttpFailure::Timeout(message) => self.alloc_string_ctor(
                    CtorRuntimeRole::PreludeIoErrorHttpTimeout,
                    &message,
                    "httpRequestRaw",
                ),
                super::http::HttpFailure::Redirect(message) => self.alloc_string_ctor(
                    CtorRuntimeRole::PreludeIoErrorHttpRedirect,
                    &message,
                    "httpRequestRaw",
                ),
                super::http::HttpFailure::HeadersTooLarge(maximum) => self.alloc_int_ctor(
                    CtorRuntimeRole::PreludeIoErrorHttpHeadersTooLarge,
                    maximum,
                    "httpRequestRaw",
                ),
                super::http::HttpFailure::BodyTooLarge(maximum) => self.alloc_int_ctor(
                    CtorRuntimeRole::PreludeIoErrorHttpBodyTooLarge,
                    maximum,
                    "httpRequestRaw",
                ),
            },
        }
    }

    fn alloc_int_ctor(
        &mut self,
        role: CtorRuntimeRole,
        value: usize,
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        let value = i64::try_from(value).map_err(|_out_of_range| {
            RuntimeError::builtin(format!("{builtin} limit exceeded Int"))
        })?;
        VmCtx::new(self).alloc_data1_ctor(role, Value::Int(value), builtin)
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

    fn alloc_io_error3(
        &mut self,
        role: CtorRuntimeRole,
        first: &str,
        second: &str,
        third: &str,
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        let checkpoint = self.temporary_roots.len();
        for field in [first, second, third] {
            self.safe_point();
            let field = self.heap.alloc_str(field);
            self.temporary_roots.push(field);
        }
        self.safe_point();
        let first = *self
            .temporary_roots
            .get(checkpoint)
            .expect("ICE: first IO error field root disappeared during allocation");
        let second = *self
            .temporary_roots
            .get(checkpoint + 1)
            .expect("ICE: second IO error field root disappeared during allocation");
        let third = *self
            .temporary_roots
            .get(checkpoint + 2)
            .expect("ICE: third IO error field root disappeared during allocation");
        let result = VmCtx::new(self).alloc_data3_ctor(role, first, second, third, builtin)?;
        self.temporary_roots.truncate(checkpoint);
        Ok(result)
    }

    pub(super) fn alloc_io_error2(
        &mut self,
        role: CtorRuntimeRole,
        first: &str,
        second: &str,
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        self.safe_point();
        let root = self.temporary_roots.len();
        let first = self.heap.alloc_str(first);
        self.temporary_roots.push(first);
        self.safe_point();
        let second = self.heap.alloc_str(second);
        self.temporary_roots.push(second);
        self.safe_point();
        let first = *self
            .temporary_roots
            .get(root)
            .expect("ICE: first IO error field root disappeared during allocation");
        let second = *self
            .temporary_roots
            .get(root + 1)
            .expect("ICE: second IO error field root disappeared during allocation");
        let result = VmCtx::new(self).alloc_data2_ctor(role, first, second, builtin);
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
