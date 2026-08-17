//! Package-aware permission checks shared by mutating editor features.

use super::*;
use thiserror::Error;

/// Explains why an editor operation cannot modify a source file.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum SourceEditError {
    /// The file belongs to a path dependency rather than the open local package.
    #[error(
        "cannot edit dependency source `{}`; open that package as the local project to change it",
        path.display()
    )]
    DependencyPackage { path: PathBuf },
    /// The file belongs to the automatically imported compiler library.
    #[error(
        "cannot edit implicit library source `{}`; open that library as the local project to change it",
        path.display()
    )]
    ImplicitPackage { path: PathBuf },
    /// The path is not one of the checked project's source files.
    #[error("cannot edit unknown project source `{}`", path.display())]
    UnknownSource { path: PathBuf },
}

pub(super) fn ensure_source_editable(
    project: &Project,
    source_map: &SourceMap,
    file: &Path,
) -> Result<(), SourceEditError> {
    let Some(file_id) = source_lookup::source_file_id_in_source_map(source_map, file) else {
        return Err(SourceEditError::UnknownSource {
            path: file.to_path_buf(),
        });
    };

    ensure_file_id_editable(project, source_map, file_id)
}

pub(super) fn ensure_file_id_editable(
    project: &Project,
    source_map: &SourceMap,
    file: FileId,
) -> Result<(), SourceEditError> {
    let path = source_map.lookup(file).path.clone();
    let kind = project
        .modules
        .values()
        .find(|module| module.parsed.file == file)
        .and_then(|module| project.package_kind(module.package));

    match kind {
        Some(PackageKind::Local) => Ok(()),
        Some(PackageKind::Dependency) => Err(SourceEditError::DependencyPackage { path }),
        Some(PackageKind::Implicit) => Err(SourceEditError::ImplicitPackage { path }),
        None => Err(SourceEditError::UnknownSource { path }),
    }
}

pub(super) fn code_action_is_editable(
    project: &Project,
    source_map: &SourceMap,
    action: &SourceCodeAction,
) -> bool {
    action
        .edits()
        .iter()
        .all(|edit| ensure_file_id_editable(project, source_map, edit.file()).is_ok())
}
