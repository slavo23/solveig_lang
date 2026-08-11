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

impl Interpreter<'_> {
    pub(super) fn perform_io(
        &mut self,
        program: &BytecodeProgram,
        action: Value,
    ) -> Result<Value, StepOutcome> {
        if let Some(result) = self.io_result.take() {
            self.io_current = None;
            let value = self.io_result_value(result)?;
            if let Some(value) = self.resume_io_value(program, value)? {
                self.continuations.clear();
                return Ok(value);
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
                IoAction::Fail(_) => return Ok(current),
                IoAction::Bind {
                    action,
                    continuation,
                } => {
                    self.continuations.push(Continuation::IoBind(continuation));
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

    fn io_result_value(&mut self, result: IoResult) -> Result<Value, RuntimeError> {
        match result {
            IoResult::Unit => Ok(Value::Unit),
            IoResult::Text(text) => {
                self.safe_point();
                Ok(self.heap.alloc_str(&text))
            }
            IoResult::Failed(failure) => Err(io_failure(failure)),
        }
    }

    fn resume_io_value(
        &mut self,
        program: &BytecodeProgram,
        value: Value,
    ) -> Result<Option<Value>, StepOutcome> {
        let Some(frame) = self.continuations.pop() else {
            return Ok(Some(value));
        };

        match frame {
            Continuation::IoBind(continuation) => {
                self.io_current = None;
                let next = self.call_value_for_result(program, continuation, &[value])?;
                self.io_current = Some(next);
                Ok(None)
            }
            Continuation::Halt
            | Continuation::CallerFrame { .. }
            | Continuation::PublishGlobalThenHalt { .. }
            | Continuation::PublishGlobalThenResume { .. } => {
                solveig_base::ice!("non-IO continuation reached the IO machine")
            }
        }
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

fn io_failure(failure: IoFailure) -> RuntimeError {
    let message = match failure {
        IoFailure::Print(error) => format!("IO write failed: {error}"),
        IoFailure::ReadLine(error) => format!("readLine failed: {error}"),
        IoFailure::ReadFile { path, source } => {
            format!("readFile failed for `{}`: {source}", path.display())
        }
        IoFailure::WriteFile {
            path,
            append,
            source,
        } => {
            let operation = if append { "appendFile" } else { "writeFile" };
            format!("{operation} failed for `{}`: {source}", path.display())
        }
    };
    RuntimeError::builtin(message)
}
