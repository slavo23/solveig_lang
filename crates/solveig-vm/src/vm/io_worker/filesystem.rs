//! Blocking filesystem requests and their operation-specific failures.

use super::*;
use std::ffi::OsString;
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
    WriteFileAtomic {
        path: PathBuf,
        contents: Vec<u8>,
        operation: &'static str,
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
    WriteFileAtomic {
        path: PathBuf,
        operation: &'static str,
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
        FilesystemRequest::WriteFileAtomic {
            path,
            contents,
            operation,
        } => write_file_atomic(path, &contents, operation),
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

fn write_file_atomic(path: PathBuf, contents: &[u8], operation: &'static str) -> IoResult {
    match replace_file_atomically(&path, contents) {
        Ok(()) => IoResult::Unit,
        Err(source) => failed(FilesystemFailure::WriteFileAtomic {
            path,
            operation,
            source,
        }),
    }
}

fn replace_file_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "atomic file destination must have a file name",
        )
    })?;
    let existing_permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());

    for attempt in 0..1024_u16 {
        let mut temporary_name = OsString::from(".");
        temporary_name.push(file_name);
        temporary_name.push(format!(".solveig-atomic-{}-{attempt}", std::process::id()));
        let temporary_path = parent.join(temporary_name);
        let mut temporary = match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };

        let result = (|| {
            if let Some(permissions) = existing_permissions.clone() {
                temporary.set_permissions(permissions)?;
            }
            temporary.write_all(contents)?;
            temporary.sync_all()?;
            drop(temporary);
            fs::rename(&temporary_path, path)
        })();
        if result.is_err() {
            let _ignored_cleanup_error = fs::remove_file(&temporary_path);
        }
        return result;
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve an atomic-write temporary file after 1024 attempts",
    ))
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
