//! Runtime IO action representation and execution.
//! Builtin IO values are interpreted here against files, stdin, stdout, and args.

use super::*;
use crate::builtins::VmCtx;
use crate::bytecode::BytecodeProgram;
use crate::heap::{HeapObject, IoAction};
use crate::values::Value;
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

impl Vm {
    pub(super) fn perform_io(
        &mut self,
        program: &BytecodeProgram,
        action: Value,
        io: &mut IoRuntime<'_>,
    ) -> Result<Value, RuntimeError> {
        match self.expect_io_action(action)? {
            IoAction::Pure(value) => Ok(value),
            IoAction::Bind {
                action,
                continuation,
            } => {
                let value = self.perform_io(program, action, io)?;
                let next = self.call_value_for_result(program, continuation, &[value])?;
                self.perform_io(program, next, io)
            }
            IoAction::Print { text, newline } => {
                let mut text = self.expect_string_value(text, "IO print")?;
                if newline {
                    text.push('\n');
                }
                io.write_str(&text)?;
                Ok(Value::Unit)
            }
            IoAction::ReadLine => {
                let line = io.read_line()?;
                Ok(self.heap.alloc_str(&line))
            }
            IoAction::GetArgs => {
                let mut arg_values = Vec::with_capacity(io.args.len());
                for arg in io.args {
                    arg_values.push(self.heap.alloc_str(arg));
                }

                let mut ctx = VmCtx::new(
                    &mut self.heap,
                    &program.constructor_arities,
                    &self.nullary_ctors,
                    self.bools.true_val,
                    self.bools.false_val,
                );
                ctx.alloc_list_from_values(&arg_values, "getArgs")
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
                Ok(self.heap.alloc_str(&contents))
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
                Ok(Value::Unit)
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
                HeapObject::Str { bytes } => Ok(self.heap.str_value(*bytes).to_string()),
                _ => Err(RuntimeError::builtin(format!("{context} expected String"))),
            },
            _ => Err(RuntimeError::builtin(format!("{context} expected String"))),
        }
    }
}
