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

pub(super) fn run_process_action(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let program = expect_value(args, 0, "runProcess")?;
    let arguments = expect_value(args, 1, "runProcess")?;
    Ok(ctx.alloc_io_action(IoAction::RunProcess {
        program,
        arguments,
        maximum: None,
    }))
}

pub(super) fn run_process_limited_action(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let program = expect_value(args, 0, "runProcessLimited")?;
    let arguments = expect_value(args, 1, "runProcessLimited")?;
    let maximum = expect_int(args, 2, "runProcessLimited")?;
    Ok(ctx.alloc_io_action(IoAction::RunProcess {
        program,
        arguments,
        maximum: Some(maximum),
    }))
}

pub(super) fn exit_with(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let status = expect_int(args, 0, "exitWith")?;
    Ok(ctx.alloc_io_action(IoAction::Exit { status }))
}
