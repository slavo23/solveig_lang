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
            FilesystemAction::WriteFileAtomic {
                path,
                contents,
                bytes,
            } => {
                let builtin = if bytes {
                    "writeFileBytesAtomic"
                } else {
                    "writeFileAtomic"
                };
                let path = self.resolve_filesystem_path(path, builtin)?;
                let contents = if bytes {
                    self.expect_bytes_value(contents, builtin)?
                } else {
                    self.expect_string_value(contents, builtin)?.into_bytes()
                };
                FilesystemRequest::WriteFileAtomic {
                    path,
                    contents,
                    operation: builtin,
                }
            }
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
        request: Value,
    ) -> Result<ProcessRequest, RuntimeError> {
        let builtin = "runProcessConfigured";
        let [program, arguments, config] = self.expect_tuple::<3>(request, builtin)?;
        let program = self.expect_string_value(program, builtin)?;
        let arguments = self.expect_string_list_value(arguments, builtin)?;
        let [
            cwd,
            inherit_environment,
            environment,
            removed_environment,
            stdin,
            capture_stdout,
            capture_stderr,
            maximum,
        ] = self.expect_tuple::<8>(config, builtin)?;
        let cwd = self.expect_string_value(cwd, builtin)?;
        let cwd = if cwd.is_empty() {
            self.io.current_directory().to_path_buf()
        } else {
            self.io.resolve_path(&cwd)
        };
        let maximum = self.expect_optional_nonnegative_int(maximum, builtin)?;
        let program_path = std::path::Path::new(&program);
        let executable = if program_path.is_absolute() || program_path.components().count() == 1 {
            program_path.to_path_buf()
        } else {
            cwd.join(program_path)
        };
        let inherit_environment = self.expect_bool_value(inherit_environment, builtin)?;
        let mut host_environment = if inherit_environment {
            self.io.environment().clone()
        } else {
            std::collections::BTreeMap::new()
        };
        for name in self.expect_string_list_value(removed_environment, builtin)? {
            host_environment.remove(&name);
        }
        for (name, value) in self.expect_string_pair_list_value(environment, builtin)? {
            host_environment.insert(name, value);
        }
        Ok(ProcessRequest {
            program,
            executable,
            arguments,
            cwd,
            environment: host_environment.into_iter().collect(),
            stdin: self.expect_bytes_value(stdin, builtin)?,
            capture_stdout: self.expect_bool_value(capture_stdout, builtin)?,
            capture_stderr: self.expect_bool_value(capture_stderr, builtin)?,
            maximum,
        })
    }

    fn expect_tuple<const N: usize>(
        &mut self,
        value: Value,
        builtin: &str,
    ) -> Result<[Value; N], RuntimeError> {
        let role_arity = u8::try_from(N)
            .map_err(|_arity| RuntimeError::builtin(format!("{builtin} tuple arity is invalid")))?;
        let ctor_arity = u16::try_from(N)
            .map_err(|_arity| RuntimeError::builtin(format!("{builtin} tuple arity is invalid")))?;
        let tag = VmCtx::new(self).ctor_tag_with_arity(
            CtorRuntimeRole::PreludeTuple(role_arity),
            ctor_arity,
            builtin,
        )?;
        let Value::Obj(id) = value else {
            return Err(RuntimeError::builtin(format!(
                "{builtin} expected a {N}-tuple"
            )));
        };
        let values = match self.heap.get(id) {
            HeapObject::DataN {
                tag: actual_tag,
                fields,
            } if *actual_tag == tag => self.heap.payload_slice(id, *fields).to_vec(),
            _ => Vec::new(),
        };
        values
            .try_into()
            .map_err(|_values| RuntimeError::builtin(format!("{builtin} expected a {N}-tuple")))
    }

    fn expect_bool_value(&self, value: Value, builtin: &str) -> Result<bool, RuntimeError> {
        if value == self.bool_value(true) {
            Ok(true)
        } else if value == self.bool_value(false) {
            Ok(false)
        } else {
            Err(RuntimeError::builtin(format!(
                "{builtin} expected a Bool configuration value"
            )))
        }
    }

    fn expect_optional_nonnegative_int(
        &mut self,
        value: Value,
        builtin: &str,
    ) -> Result<Option<i64>, RuntimeError> {
        let nothing_tag = VmCtx::new(self).ctor_tag_with_arity(
            CtorRuntimeRole::PreludeMaybeNothing,
            0,
            builtin,
        )?;
        let just_tag =
            VmCtx::new(self).ctor_tag_with_arity(CtorRuntimeRole::PreludeMaybeJust, 1, builtin)?;
        let Value::Obj(id) = value else {
            return Err(RuntimeError::builtin(format!(
                "{builtin} expected Maybe Int for the output limit"
            )));
        };
        match self.heap.get(id) {
            HeapObject::Data0 { tag } if *tag == nothing_tag => Ok(None),
            HeapObject::Data1 {
                tag,
                field: Value::Int(maximum),
            } if *tag == just_tag && *maximum >= 0 => Ok(Some(*maximum)),
            _ => Err(RuntimeError::builtin(format!(
                "{builtin} expected Nothing or a non-negative Just Int output limit"
            ))),
        }
    }

    fn expect_string_pair_list_value(
        &mut self,
        mut current: Value,
        builtin: &str,
    ) -> Result<Vec<(String, String)>, RuntimeError> {
        let nil_tag =
            VmCtx::new(self).ctor_tag_with_arity(CtorRuntimeRole::PreludeListNil, 0, builtin)?;
        let cons_tag =
            VmCtx::new(self).ctor_tag_with_arity(CtorRuntimeRole::PreludeListCons, 2, builtin)?;
        let pair_tag =
            VmCtx::new(self).ctor_tag_with_arity(CtorRuntimeRole::PreludeTuple(2), 2, builtin)?;
        let mut pairs = Vec::new();

        loop {
            let Value::Obj(id) = current else {
                return Err(RuntimeError::builtin(format!(
                    "{builtin} expected [(String, String)]"
                )));
            };
            match self.heap.get(id) {
                HeapObject::Data0 { tag } if *tag == nil_tag => return Ok(pairs),
                HeapObject::Data2 {
                    tag,
                    field0,
                    field1,
                } if *tag == cons_tag => {
                    let pair = *field0;
                    current = *field1;
                    let Value::Obj(pair_id) = pair else {
                        return Err(RuntimeError::builtin(format!(
                            "{builtin} expected environment pairs"
                        )));
                    };
                    let HeapObject::Data2 {
                        tag,
                        field0,
                        field1,
                    } = self.heap.get(pair_id)
                    else {
                        return Err(RuntimeError::builtin(format!(
                            "{builtin} expected environment pairs"
                        )));
                    };
                    if *tag != pair_tag {
                        return Err(RuntimeError::builtin(format!(
                            "{builtin} expected environment pairs"
                        )));
                    }
                    pairs.push((
                        self.expect_string_value(*field0, builtin)?,
                        self.expect_string_value(*field1, builtin)?,
                    ));
                }
                _ => {
                    return Err(RuntimeError::builtin(format!(
                        "{builtin} expected [(String, String)]"
                    )));
                }
            }
        }
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
