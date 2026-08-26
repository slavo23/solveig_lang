//! Pure platform path operations used by the opaque `std.path.Path` facade.

use std::path::{Component, Path, PathBuf};

use solveig_ast::ast::CtorRuntimeRole;

use super::VmCtx;
use super::text::expect_string_owned;
use crate::heap::HeapObject;
use crate::values::Value;
use crate::vm::RuntimeError;

pub(super) fn path_join(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let base = expect_string_owned(ctx, args, 0, "pathJoin")?;
    let child = expect_string_owned(ctx, args, 1, "pathJoin")?;
    let mut joined = PathBuf::from(base);
    joined.push(child);
    alloc_path_string(ctx, &joined, "pathJoin")
}

pub(super) fn path_parent(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string_owned(ctx, args, 0, "pathParent")?;
    let parent = Path::new(&path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf);
    alloc_optional_path_string(ctx, parent.as_deref(), "pathParent")
}

pub(super) fn path_file_name(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let path = expect_string_owned(ctx, args, 0, "pathFileName")?;
    alloc_optional_os_string(ctx, Path::new(&path).file_name(), "pathFileName")
}

pub(super) fn path_extension(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let path = expect_string_owned(ctx, args, 0, "pathExtension")?;
    alloc_optional_os_string(ctx, Path::new(&path).extension(), "pathExtension")
}

pub(super) fn path_normalise(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let path = expect_string_owned(ctx, args, 0, "pathNormalise")?;
    alloc_path_string(ctx, &normalise(Path::new(&path)), "pathNormalise")
}

pub(super) fn path_is_absolute(
    ctx: &mut VmCtx<'_, '_>,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let path = expect_string_owned(ctx, args, 0, "pathIsAbsolute")?;
    Ok(ctx.bool_value(Path::new(&path).is_absolute()))
}

fn normalise(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => result.push(prefix.as_os_str()),
            Component::RootDir => result.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => match result.components().next_back() {
                Some(Component::Normal(_)) => {
                    let _removed = result.pop();
                }
                Some(Component::RootDir) => {}
                _ if result.has_root() => {}
                _ => result.push(component.as_os_str()),
            },
            Component::Normal(part) => result.push(part),
        }
    }

    if result.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        result
    }
}

fn alloc_optional_path_string(
    ctx: &mut VmCtx<'_, '_>,
    path: Option<&Path>,
    builtin: &str,
) -> Result<Value, RuntimeError> {
    match path {
        Some(path) => alloc_optional_string(ctx, Some(path_to_utf8(path, builtin)?), builtin),
        None => alloc_optional_string(ctx, None, builtin),
    }
}

fn alloc_optional_os_string(
    ctx: &mut VmCtx<'_, '_>,
    value: Option<&std::ffi::OsStr>,
    builtin: &str,
) -> Result<Value, RuntimeError> {
    match value {
        Some(value) => {
            let value = value.to_str().ok_or_else(|| {
                RuntimeError::builtin(format!("{builtin} produced a non-UTF-8 path component"))
            })?;
            alloc_optional_string(ctx, Some(value), builtin)
        }
        None => alloc_optional_string(ctx, None, builtin),
    }
}

fn alloc_optional_string(
    ctx: &mut VmCtx<'_, '_>,
    value: Option<&str>,
    builtin: &str,
) -> Result<Value, RuntimeError> {
    let Some(value) = value else {
        return ctx.nullary_ctor(CtorRuntimeRole::PreludeMaybeNothing, builtin);
    };
    let tag = ctx.ctor_tag_with_arity(CtorRuntimeRole::PreludeMaybeJust, 1, builtin)?;
    let checkpoint = ctx.interpreter.process.temporary_roots.len();
    let value = ctx.alloc_str(value);
    ctx.interpreter.process.temporary_roots.push(value);
    ctx.interpreter.safe_point();
    let value = ctx.temporary_root(checkpoint, builtin, "path text");
    let result = ctx
        .interpreter
        .heap
        .alloc(HeapObject::Data1 { tag, field: value });
    ctx.interpreter.process.temporary_roots.truncate(checkpoint);
    Ok(result)
}

fn alloc_path_string(
    ctx: &mut VmCtx<'_, '_>,
    path: &Path,
    builtin: &str,
) -> Result<Value, RuntimeError> {
    Ok(ctx.alloc_str(path_to_utf8(path, builtin)?))
}

fn path_to_utf8<'path>(path: &'path Path, builtin: &str) -> Result<&'path str, RuntimeError> {
    path.to_str().ok_or_else(|| {
        RuntimeError::builtin(format!("{builtin} produced a non-UTF-8 path spelling"))
    })
}
