//! Blocking filesystem requests and their operation-specific failures.

use super::*;
use std::time::UNIX_EPOCH;

pub(crate) enum FilesystemRequest {
    SetCurrentDirectory(PathBuf),
    DoesFileExist(PathBuf),
    DoesDirectoryExist(PathBuf),
    ListDirectory(PathBuf),
    CreateDirectory(PathBuf),
    RemoveFile(PathBuf),
    RenameFile {
        source: PathBuf,
        destination: PathBuf,
    },
    GetFileSize(PathBuf),
    GetModificationTime(PathBuf),
}

pub(crate) enum FilesystemFailure {
    GetCurrentDirectory(io::Error),
    SetCurrentDirectory {
        path: PathBuf,
        source: io::Error,
    },
    DoesFileExist {
        path: PathBuf,
        source: io::Error,
    },
    DoesDirectoryExist {
        path: PathBuf,
        source: io::Error,
    },
    ListDirectory {
        path: PathBuf,
        source: io::Error,
    },
    CreateDirectory {
        path: PathBuf,
        source: io::Error,
    },
    RemoveFile {
        path: PathBuf,
        source: io::Error,
    },
    RenameFile {
        source_path: PathBuf,
        destination: PathBuf,
        source: io::Error,
    },
    GetFileSize {
        path: PathBuf,
        source: io::Error,
    },
    GetModificationTime {
        path: PathBuf,
        source: io::Error,
    },
}

pub(super) fn perform(request: FilesystemRequest) -> IoResult {
    match request {
        FilesystemRequest::SetCurrentDirectory(path) => set_current_directory(path),
        FilesystemRequest::DoesFileExist(path) => path_kind(path, true),
        FilesystemRequest::DoesDirectoryExist(path) => path_kind(path, false),
        FilesystemRequest::ListDirectory(path) => list_directory(path),
        FilesystemRequest::CreateDirectory(path) => unit_path(
            path,
            |path| fs::create_dir(path),
            |path, source| FilesystemFailure::CreateDirectory { path, source },
        ),
        FilesystemRequest::RemoveFile(path) => unit_path(
            path,
            |path| fs::remove_file(path),
            |path, source| FilesystemFailure::RemoveFile { path, source },
        ),
        FilesystemRequest::RenameFile {
            source,
            destination,
        } => rename_file(source, destination),
        FilesystemRequest::GetFileSize(path) => get_file_size(path),
        FilesystemRequest::GetModificationTime(path) => get_modification_time(path),
    }
}

fn set_current_directory(path: PathBuf) -> IoResult {
    let result = fs::canonicalize(&path).and_then(|resolved| {
        if resolved.is_dir() {
            Ok(resolved)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "path is not a directory",
            ))
        }
    });
    match result {
        Ok(path) => IoResult::DirectoryChanged(path),
        Err(source) => failed(FilesystemFailure::SetCurrentDirectory { path, source }),
    }
}

fn path_kind(path: PathBuf, file: bool) -> IoResult {
    match fs::metadata(&path) {
        Ok(metadata) => IoResult::Bool(if file {
            metadata.is_file()
        } else {
            metadata.is_dir()
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => IoResult::Bool(false),
        Err(source) => failed(if file {
            FilesystemFailure::DoesFileExist { path, source }
        } else {
            FilesystemFailure::DoesDirectoryExist { path, source }
        }),
    }
}

fn list_directory(path: PathBuf) -> IoResult {
    let result = fs::read_dir(&path).and_then(|entries| {
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().into_string().map_err(|_non_utf8| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "directory entry name is not valid UTF-8",
                )
            })?;
            names.push(name);
        }
        names.sort_unstable();
        Ok(names)
    });
    match result {
        Ok(names) => IoResult::TextList(names),
        Err(source) => failed(FilesystemFailure::ListDirectory { path, source }),
    }
}

fn unit_path(
    path: PathBuf,
    operation: impl FnOnce(&Path) -> io::Result<()>,
    failure: impl FnOnce(PathBuf, io::Error) -> FilesystemFailure,
) -> IoResult {
    match operation(&path) {
        Ok(()) => IoResult::Unit,
        Err(source) => failed(failure(path, source)),
    }
}

fn rename_file(source_path: PathBuf, destination: PathBuf) -> IoResult {
    match fs::rename(&source_path, &destination) {
        Ok(()) => IoResult::Unit,
        Err(source) => failed(FilesystemFailure::RenameFile {
            source_path,
            destination,
            source,
        }),
    }
}

fn get_file_size(path: PathBuf) -> IoResult {
    let result = fs::metadata(&path).and_then(|metadata| {
        i64::try_from(metadata.len())
            .map_err(|_too_large| io::Error::other("file size exceeds Int range"))
    });
    match result {
        Ok(size) => IoResult::Int(size),
        Err(source) => failed(FilesystemFailure::GetFileSize { path, source }),
    }
}

fn get_modification_time(path: PathBuf) -> IoResult {
    let result = fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .and_then(|modified| {
            modified.duration_since(UNIX_EPOCH).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("modification time predates the Unix epoch: {error}"),
                )
            })
        })
        .and_then(|duration| {
            i64::try_from(duration.as_nanos()).map_err(|_too_large| {
                io::Error::other("modification time exceeds nanosecond Int range")
            })
        });
    match result {
        Ok(nanoseconds) => IoResult::Int(nanoseconds),
        Err(source) => failed(FilesystemFailure::GetModificationTime { path, source }),
    }
}

fn failed(failure: FilesystemFailure) -> IoResult {
    IoResult::Failed(IoFailure::Filesystem(failure))
}
