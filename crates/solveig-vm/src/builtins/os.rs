//! Construction of inert environment and filesystem actions.

use super::*;
use crate::heap::{EnvironmentAction, FilesystemAction};

pub(super) fn get_env(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let name = expect_value(args, 0, "getEnv")?;
    Ok(ctx.alloc_io_action(IoAction::Environment(EnvironmentAction::Get { name })))
}

pub(super) fn set_env(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let name = expect_value(args, 0, "setEnv")?;
    let value = expect_value(args, 1, "setEnv")?;
    Ok(
        ctx.alloc_io_action(IoAction::Environment(EnvironmentAction::Set {
            name,
            value,
        })),
    )
}

pub(super) fn get_environment(
    ctx: &mut VmCtx<'_, '_>,
    _args: &[Value],
) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_io_action(IoAction::Environment(EnvironmentAction::GetAll)))
}

pub(super) fn get_prog_name(
    ctx: &mut VmCtx<'_, '_>,
    _args: &[Value],
) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_io_action(IoAction::Environment(EnvironmentAction::GetProgramName)))
}

macro_rules! path_action {
    ($function:ident, $builtin:literal, $variant:ident) => {
        pub(super) fn $function(
            ctx: &mut VmCtx<'_, '_>,
            args: &[Value],
        ) -> Result<Value, RuntimeError> {
            let path = expect_value(args, 0, $builtin)?;
            Ok(ctx.alloc_io_action(IoAction::Filesystem(FilesystemAction::$variant { path })))
        }
    };
}

pub(super) fn get_current_directory(
    ctx: &mut VmCtx<'_, '_>,
    _args: &[Value],
) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_io_action(IoAction::Filesystem(FilesystemAction::GetCurrentDirectory)))
}

path_action!(
    set_current_directory,
    "setCurrentDirectory",
    SetCurrentDirectory
);
path_action!(does_file_exist, "doesFileExist", DoesFileExist);
path_action!(
    does_directory_exist,
    "doesDirectoryExist",
    DoesDirectoryExist
);
path_action!(list_directory, "listDirectory", ListDirectory);
path_action!(create_directory, "createDirectory", CreateDirectory);
path_action!(remove_file, "removeFile", RemoveFile);
path_action!(get_file_size, "getFileSize", GetFileSize);
path_action!(
    get_modification_time,
    "getModificationTime",
    GetModificationTime
);

pub(super) fn rename_file(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let source = expect_value(args, 0, "renameFile")?;
    let destination = expect_value(args, 1, "renameFile")?;
    Ok(
        ctx.alloc_io_action(IoAction::Filesystem(FilesystemAction::RenameFile {
            source,
            destination,
        })),
    )
}

pub(super) fn run_process_configured_action(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let builtin = "runProcessConfigured";
    let request = expect_value(args, 0, builtin)?;
    Ok(ctx.alloc_io_action(IoAction::RunProcess { request }))
}

pub(super) fn start_process_configured_action(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let builtin = "startProcessConfigured";
    let request = expect_value(args, 0, builtin)?;
    Ok(ctx.alloc_io_action(IoAction::StartProcess { request }))
}

macro_rules! process_action {
    ($function:ident, $builtin:literal, $variant:ident) => {
        pub(super) fn $function(
            ctx: &mut VmCtx<'_, '_>,
            args: &[Value],
        ) -> Result<Value, RuntimeError> {
            let process = expect_value(args, 0, $builtin)?;
            Ok(ctx.alloc_io_action(IoAction::$variant { process }))
        }
    };
}

pub(super) fn read_process_stdout_action(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let process = expect_value(args, 0, "readProcessStdout")?;
    let maximum = expect_int(args, 1, "readProcessStdout")?;
    Ok(ctx.alloc_io_action(IoAction::ReadProcessStdout { process, maximum }))
}

pub(super) fn read_process_stderr_action(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let process = expect_value(args, 0, "readProcessStderr")?;
    let maximum = expect_int(args, 1, "readProcessStderr")?;
    Ok(ctx.alloc_io_action(IoAction::ReadProcessStderr { process, maximum }))
}

pub(super) fn write_process_stdin_action(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let process = expect_value(args, 0, "writeProcessStdin")?;
    let bytes = expect_value(args, 1, "writeProcessStdin")?;
    Ok(ctx.alloc_io_action(IoAction::WriteProcessStdin { process, bytes }))
}

process_action!(
    close_process_stdin_action,
    "closeProcessStdin",
    CloseProcessStdin
);
process_action!(wait_process_action, "waitProcess", WaitProcess);
process_action!(
    terminate_process_action,
    "terminateProcess",
    TerminateProcess
);
process_action!(close_process_action, "closeProcess", CloseProcess);

pub(super) fn exit_with(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let status = expect_int(args, 0, "exitWith")?;
    Ok(ctx.alloc_io_action(IoAction::Exit { status }))
}
