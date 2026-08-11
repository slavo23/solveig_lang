//! Runtime IO action representation and execution.
//! The current action stays in one register while suspended binds share the VM's
//! continuation stack. Blocking actions park the process until an IO worker
//! returns plain Rust data for scheduler-thread conversion.

use super::io_worker::{IoFailure, IoRequest, IoResult};
use super::*;
use crate::builtins::VmCtx;
use crate::bytecode::BytecodeProgram;
use crate::heap::{HeapObject, IoAction};
use crate::values::{Continuation, Value};
use solveig_ast::ast::CtorRuntimeRole;

impl Interpreter<'_> {
    pub(super) fn perform_io(
        &mut self,
        program: &BytecodeProgram,
        action: Value,
    ) -> Result<Value, StepOutcome> {
        if let Some(result) = self.io_result.take() {
            self.io_current = None;
            match result {
                IoResult::Unit => {
                    if let Some(value) = self.resume_io_value(program, Value::Unit)? {
                        self.continuations.clear();
                        return Ok(value);
                    }
                }
                IoResult::Text(text) => {
                    self.safe_point();
                    let value = self.heap.alloc_str(&text);
                    if let Some(value) = self.resume_io_value(program, value)? {
                        self.continuations.clear();
                        return Ok(value);
                    }
                }
                IoResult::Failed(failure) => {
                    self.io_current = Some(self.alloc_io_failure_action(failure)?);
                }
            }
        }

        if self.io_current.is_none() {
            if self.continuations.is_empty() {
                self.io_current = Some(action);
            } else {
                self.io_current = Some(self.dispatch(program)?);
            }
        }

        let result = self.perform_io_loop(program);

        if result.is_ok() {
            self.io_current = None;
            self.continuations.clear();
        }
        result
    }

    fn perform_io_loop(&mut self, program: &BytecodeProgram) -> Result<Value, StepOutcome> {
        // Awkward Squad section 2 gives IO bind the sequencing contract; AOSA's
        // GHC RTS chapter keeps resumable runtime state explicit at safe points.
        loop {
            let current = self
                .io_current
                .expect("ICE: IO machine missing current action");

            match self.expect_io_action(current)? {
                IoAction::Pure(value) => {
                    if let Some(value) = self.resume_io_value(program, value)? {
                        return Ok(value);
                    }
                }
                IoAction::Fail(error) => {
                    if let Some(value) = self.resume_io_failure(program, current, error)? {
                        return Ok(value);
                    }
                }
                IoAction::Bind {
                    action,
                    continuation,
                } => {
                    self.continuations.push(Continuation::IoBind(continuation));
                    self.io_current = Some(action);
                }
                IoAction::Catch { action, handler } => {
                    self.continuations.push(Continuation::IoCatch(handler));
                    self.io_current = Some(action);
                }
                IoAction::Finally { action, cleanup } => {
                    self.continuations.push(Continuation::IoFinally(cleanup));
                    self.io_current = Some(action);
                }
                IoAction::Print { text, newline } => {
                    let mut text = self.expect_string_value(text, "IO print")?;
                    if newline {
                        text.push('\n');
                    }
                    return self.block_on_io(IoRequest::Print(text));
                }
                IoAction::ReadLine => {
                    return self.block_on_io(IoRequest::ReadLine);
                }
                IoAction::GetArgs => {
                    let checkpoint = self.temporary_roots.len();
                    let arg_count = self.io.args().len();
                    for index in 0..arg_count {
                        let Some(arg) = self.io.args().get(index).cloned() else {
                            solveig_base::ice!("IO argument index disappeared during allocation");
                        };
                        self.safe_point();
                        let value = self.heap.alloc_str(&arg);
                        self.temporary_roots.push(value);
                    }

                    let mut ctx = VmCtx::new(self);
                    let value = ctx.alloc_list_from_root_range(checkpoint, arg_count, "getArgs");
                    self.temporary_roots.truncate(checkpoint);
                    let value = value?;
                    if let Some(value) = self.resume_io_value(program, value)? {
                        return Ok(value);
                    }
                }
                IoAction::Spawn { action } => {
                    let thread_id = self.spawn_io_process(action);
                    if let Some(value) = self.resume_io_value(program, thread_id)? {
                        return Ok(value);
                    }
                }
                IoAction::ReadFile { path } => {
                    let path = self.expect_string_value(path, "readFile")?;
                    let resolved = self.io.resolve_path(&path);
                    return self.block_on_io(IoRequest::ReadFile(resolved));
                }
                IoAction::WriteFile {
                    path,
                    contents,
                    append,
                } => {
                    let builtin = if append { "appendFile" } else { "writeFile" };
                    let path = self.expect_string_value(path, builtin)?;
                    let contents = self.expect_string_value(contents, builtin)?;
                    let resolved = self.io.resolve_path(&path);
                    return self.block_on_io(IoRequest::WriteFile {
                        path: resolved,
                        contents,
                        append,
                    });
                }
            }
        }
    }

    fn block_on_io(&mut self, request: IoRequest) -> Result<Value, StepOutcome> {
        let request_id = self.io.issue(self.id, request)?;
        self.status = ProcStatus::Blocked(BlockReason::Io(request_id));
        Err(StepOutcome::Blocked)
    }

    fn resume_io_value(
        &mut self,
        program: &BytecodeProgram,
        value: Value,
    ) -> Result<Option<Value>, StepOutcome> {
        loop {
            let Some(frame) = self.continuations.pop() else {
                return Ok(Some(value));
            };

            match frame {
                Continuation::IoBind(continuation) => {
                    self.io_current = None;
                    let next = self.call_value_for_result(program, continuation, &[value])?;
                    self.io_current = Some(next);
                    return Ok(None);
                }
                Continuation::IoCatch(_) => {}
                Continuation::IoFinally(cleanup) => {
                    let outcome = self.alloc_io_success_action(value);
                    self.continuations.push(Continuation::IoRestore(outcome));
                    self.io_current = Some(cleanup);
                    return Ok(None);
                }
                Continuation::IoRestore(outcome) => {
                    self.io_current = Some(outcome);
                    return Ok(None);
                }
                Continuation::Halt
                | Continuation::CallerFrame { .. }
                | Continuation::PublishGlobalThenHalt { .. }
                | Continuation::PublishGlobalThenResume { .. } => {
                    solveig_base::ice!("non-IO continuation reached the IO machine")
                }
            }
        }
    }

    fn resume_io_failure(
        &mut self,
        program: &BytecodeProgram,
        action: Value,
        error: Value,
    ) -> Result<Option<Value>, StepOutcome> {
        loop {
            let Some(frame) = self.continuations.pop() else {
                return Ok(Some(action));
            };

            match frame {
                Continuation::IoBind(_) | Continuation::IoRestore(_) => {}
                Continuation::IoCatch(handler) => {
                    self.io_current = None;
                    let next = self.call_value_for_result(program, handler, &[error])?;
                    self.io_current = Some(next);
                    return Ok(None);
                }
                Continuation::IoFinally(cleanup) => {
                    self.continuations.push(Continuation::IoRestore(action));
                    self.io_current = Some(cleanup);
                    return Ok(None);
                }
                Continuation::Halt
                | Continuation::CallerFrame { .. }
                | Continuation::PublishGlobalThenHalt { .. }
                | Continuation::PublishGlobalThenResume { .. } => {
                    solveig_base::ice!("non-IO continuation reached the IO machine")
                }
            }
        }
    }

    fn alloc_io_success_action(&mut self, value: Value) -> Value {
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

    fn alloc_io_error(&mut self, failure: IoFailure) -> Result<Value, RuntimeError> {
        match failure {
            IoFailure::Print(error) => self.alloc_io_error1(
                CtorRuntimeRole::PreludeIoErrorStdout,
                &error.to_string(),
                "print",
            ),
            IoFailure::ReadLine(error) => self.alloc_io_error1(
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
        }
    }

    fn alloc_io_error1(
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

    fn alloc_io_error2(
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
}
