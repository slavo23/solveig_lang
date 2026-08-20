//! Runtime-local environment operations and filesystem request preparation.
//! Host filesystem calls remain worker-owned; this module only decodes values,
//! allocates immediate results, and resolves paths against the logical cwd.

use super::io_worker::IoFailure;
use super::io_worker::filesystem::{FilesystemFailure, FilesystemRequest};
use super::io_worker::process::ProcessRequest;
use super::*;
use crate::builtins::VmCtx;
use crate::heap::{EnvironmentAction, FilesystemAction, HeapObject};
use crate::values::Value;
use solveig_ast::ast::CtorRuntimeRole;

pub(super) enum EnvironmentOutcome {
    Value(Value),
    Failure(IoFailure),
}

pub(super) enum FilesystemOutcome {
    Value(Value),
    Failure(IoFailure),
    Request(FilesystemRequest),
}

impl Interpreter<'_> {
    pub(super) fn perform_environment_action(
        &mut self,
        action: EnvironmentAction,
    ) -> Result<EnvironmentOutcome, RuntimeError> {
        match action {
            EnvironmentAction::Get { name } => {
                let name = self.expect_string_value(name, "getEnv")?;
                match self
                    .io
                    .get_env(&name)
                    .map(|value| value.map(str::to_string))
                {
                    Ok(Some(value)) => {
                        self.safe_point();
                        let value = self.heap.alloc_str(&value);
                        Ok(EnvironmentOutcome::Value(
                            self.alloc_maybe_value(Some(value))?,
                        ))
                    }
                    Ok(None) => Ok(EnvironmentOutcome::Value(self.alloc_maybe_value(None)?)),
                    Err(failure) => Ok(EnvironmentOutcome::Failure(failure)),
                }
            }
            EnvironmentAction::Set { name, value } => {
                let name = self.expect_string_value(name, "setEnv")?;
                let value = self.expect_string_value(value, "setEnv")?;
                match self.io.set_env(name, value) {
                    Ok(()) => Ok(EnvironmentOutcome::Value(Value::Unit)),
                    Err(failure) => Ok(EnvironmentOutcome::Failure(failure)),
                }
            }
            EnvironmentAction::GetAll => {
                let entries = self
                    .io
                    .environment()
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect::<Vec<_>>();
                Ok(EnvironmentOutcome::Value(
                    self.alloc_environment_list(&entries)?,
                ))
            }
            EnvironmentAction::GetProgramName => {
                let program_name = self.io.program_name().to_string();
                self.safe_point();
                Ok(EnvironmentOutcome::Value(
                    self.heap.alloc_str(&program_name),
                ))
            }
        }
    }

    pub(super) fn prepare_filesystem_action(
        &mut self,
        action: FilesystemAction,
    ) -> Result<FilesystemOutcome, RuntimeError> {
        let request = match action {
            FilesystemAction::GetCurrentDirectory => {
                let Some(path) = self.io.current_directory().to_str().map(str::to_string) else {
                    return Ok(FilesystemOutcome::Failure(IoFailure::Filesystem(
                        FilesystemFailure::GetCurrentDirectory(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "current directory is not valid UTF-8",
                        )),
                    )));
                };
                self.safe_point();
                return Ok(FilesystemOutcome::Value(self.heap.alloc_str(&path)));
            }
            FilesystemAction::SetCurrentDirectory { path } => {
                FilesystemRequest::SetCurrentDirectory(
                    self.resolve_filesystem_path(path, "setCurrentDirectory")?,
                )
            }
            FilesystemAction::DoesFileExist { path } => FilesystemRequest::DoesFileExist(
                self.resolve_filesystem_path(path, "doesFileExist")?,
            ),
            FilesystemAction::DoesDirectoryExist { path } => FilesystemRequest::DoesDirectoryExist(
                self.resolve_filesystem_path(path, "doesDirectoryExist")?,
            ),
            FilesystemAction::ListDirectory { path } => FilesystemRequest::ListDirectory(
                self.resolve_filesystem_path(path, "listDirectory")?,
            ),
            FilesystemAction::CreateDirectory { path } => FilesystemRequest::CreateDirectory(
                self.resolve_filesystem_path(path, "createDirectory")?,
            ),
            FilesystemAction::RemoveFile { path } => {
                FilesystemRequest::RemoveFile(self.resolve_filesystem_path(path, "removeFile")?)
            }
            FilesystemAction::RenameFile {
                source,
                destination,
            } => FilesystemRequest::RenameFile {
                source: self.resolve_filesystem_path(source, "renameFile")?,
                destination: self.resolve_filesystem_path(destination, "renameFile")?,
            },
            FilesystemAction::GetFileSize { path } => {
                FilesystemRequest::GetFileSize(self.resolve_filesystem_path(path, "getFileSize")?)
            }
            FilesystemAction::GetModificationTime { path } => {
                FilesystemRequest::GetModificationTime(
                    self.resolve_filesystem_path(path, "getModificationTime")?,
                )
            }
        };
        Ok(FilesystemOutcome::Request(request))
    }

    pub(super) fn prepare_process_request(
        &mut self,
        program: Value,
        arguments: Value,
        maximum: Option<i64>,
    ) -> Result<ProcessRequest, RuntimeError> {
        let builtin = if maximum.is_some() {
            "runProcessLimited"
        } else {
            "runProcess"
        };
        let program = self.expect_string_value(program, builtin)?;
        let arguments = self.expect_string_list_value(arguments, builtin)?;
        let program_path = std::path::Path::new(&program);
        let executable = if program_path.is_absolute() || program_path.components().count() == 1 {
            program_path.to_path_buf()
        } else {
            self.io.resolve_path(&program)
        };
        Ok(ProcessRequest {
            program,
            executable,
            arguments,
            cwd: self.io.current_directory().to_path_buf(),
            environment: self
                .io
                .environment()
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
            maximum,
        })
    }

    fn expect_string_list_value(
        &mut self,
        mut current: Value,
        builtin: &str,
    ) -> Result<Vec<String>, RuntimeError> {
        let nil_tag =
            VmCtx::new(self).ctor_tag_with_arity(CtorRuntimeRole::PreludeListNil, 0, builtin)?;
        let cons_tag =
            VmCtx::new(self).ctor_tag_with_arity(CtorRuntimeRole::PreludeListCons, 2, builtin)?;
        let mut strings = Vec::new();

        loop {
            let Value::Obj(id) = current else {
                return Err(RuntimeError::builtin(format!(
                    "{builtin} expected [String] list spine"
                )));
            };
            match self.heap.get(id) {
                HeapObject::Data0 { tag } if *tag == nil_tag => return Ok(strings),
                HeapObject::Data2 {
                    tag,
                    field0,
                    field1,
                } if *tag == cons_tag => {
                    strings.push(self.expect_string_value(*field0, builtin)?);
                    current = *field1;
                }
                _ => {
                    return Err(RuntimeError::builtin(format!(
                        "{builtin} expected [String] list spine"
                    )));
                }
            }
        }
    }

    fn resolve_filesystem_path(
        &self,
        path: Value,
        builtin: &str,
    ) -> Result<std::path::PathBuf, RuntimeError> {
        let path = self.expect_string_value(path, builtin)?;
        Ok(self.io.resolve_path(&path))
    }
}
