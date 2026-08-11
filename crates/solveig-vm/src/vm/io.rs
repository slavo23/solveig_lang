//! Runtime IO action representation and execution.
//! The current action stays in one register while suspended binds share the VM's
//! continuation stack. Builtin actions operate on files, stdin, stdout, and args.

use super::*;
use crate::builtins::VmCtx;
use crate::bytecode::BytecodeProgram;
use crate::heap::{HeapObject, IoAction};
use crate::values::{Continuation, Value};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

pub(super) struct IoRuntime<'a> {
    stdin: &'a mut dyn BufRead,
    stdout: &'a mut dyn Write,
    args: &'a [String],
    cwd: &'a Path,
    line_buffer: String,
}

impl<'a> IoRuntime<'a> {
    pub(super) fn new(
        stdin: &'a mut dyn BufRead,
        stdout: &'a mut dyn Write,
        args: &'a [String],
        cwd: &'a Path,
    ) -> Self {
        Self {
            stdin,
            stdout,
            args,
            cwd,
            line_buffer: String::new(),
        }
    }

    fn read_line(&mut self) -> Result<String, RuntimeError> {
        self.line_buffer.clear();
        let bytes = self
            .stdin
            .read_line(&mut self.line_buffer)
            .map_err(|err| RuntimeError::builtin(format!("readLine failed: {err}")))?;
        if bytes == 0 {
            return Ok(String::new());
        }

        if self.line_buffer.ends_with('\n') {
            self.line_buffer.pop();
            if self.line_buffer.ends_with('\r') {
                self.line_buffer.pop();
            }
        }

        Ok(self.line_buffer.clone())
    }

    fn write_str(&mut self, text: &str) -> Result<(), RuntimeError> {
        self.stdout
            .write_all(text.as_bytes())
            .and_then(|()| self.stdout.flush())
            .map_err(|err| RuntimeError::builtin(format!("IO write failed: {err}")))
    }

    fn resolve_path(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        }
    }
}

impl Interpreter<'_> {
    pub(super) fn perform_io(
        &mut self,
        program: &BytecodeProgram,
        action: Value,
        io: &mut IoRuntime<'_>,
    ) -> Result<Value, RuntimeError> {
        if self.io_current.is_some() || !self.continuations.is_empty() {
            solveig_base::ice!("IO machine entered while already active");
        }

        self.io_current = Some(action);
        let result = self.perform_io_loop(program, io);

        self.io_current = None;
        self.continuations.clear();
        result
    }

    fn perform_io_loop(
        &mut self,
        program: &BytecodeProgram,
        io: &mut IoRuntime<'_>,
    ) -> Result<Value, RuntimeError> {
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
                    io.write_str(&text)?;
                    if let Some(value) = self.resume_io_value(program, Value::Unit)? {
                        return Ok(value);
                    }
                }
                IoAction::ReadLine => {
                    let line = io.read_line()?;
                    self.maybe_gc();
                    let value = self.heap.alloc_str(&line);
                    if let Some(value) = self.resume_io_value(program, value)? {
                        return Ok(value);
                    }
                }
                IoAction::GetArgs => {
                    let checkpoint = self.temporary_roots.len();
                    for arg in io.args {
                        self.maybe_gc();
                        let value = self.heap.alloc_str(arg);
                        self.temporary_roots.push(value);
                    }

                    let mut ctx = VmCtx::new(self);
                    let value =
                        ctx.alloc_list_from_root_range(checkpoint, io.args.len(), "getArgs");
                    self.temporary_roots.truncate(checkpoint);
                    let value = value?;
                    if let Some(value) = self.resume_io_value(program, value)? {
                        return Ok(value);
                    }
                }
                IoAction::ReadFile { path } => {
                    let path = self.expect_string_value(path, "readFile")?;
                    let resolved = io.resolve_path(&path);
                    let contents = fs::read_to_string(&resolved).map_err(|err| {
                        RuntimeError::builtin(format!(
                            "readFile failed for `{}`: {err}",
                            resolved.display()
                        ))
                    })?;
                    self.maybe_gc();
                    let value = self.heap.alloc_str(&contents);
                    if let Some(value) = self.resume_io_value(program, value)? {
                        return Ok(value);
                    }
                }
                IoAction::WriteFile {
                    path,
                    contents,
                    append,
                } => {
                    let builtin = if append { "appendFile" } else { "writeFile" };
                    let path = self.expect_string_value(path, builtin)?;
                    let contents = self.expect_string_value(contents, builtin)?;
                    let resolved = io.resolve_path(&path);
                    if append {
                        OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&resolved)
                            .and_then(|mut file| file.write_all(contents.as_bytes()))
                            .map_err(|err| {
                                RuntimeError::builtin(format!(
                                    "appendFile failed for `{}`: {err}",
                                    resolved.display()
                                ))
                            })?;
                    } else {
                        fs::write(&resolved, contents).map_err(|err| {
                            RuntimeError::builtin(format!(
                                "writeFile failed for `{}`: {err}",
                                resolved.display()
                            ))
                        })?;
                    }
                    if let Some(value) = self.resume_io_value(program, Value::Unit)? {
                        return Ok(value);
                    }
                }
            }
        }
    }

    fn resume_io_value(
        &mut self,
        program: &BytecodeProgram,
        value: Value,
    ) -> Result<Option<Value>, RuntimeError> {
        let Some(frame) = self.continuations.pop() else {
            return Ok(Some(value));
        };

        match frame {
            Continuation::IoBind(continuation) => {
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
