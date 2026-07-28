use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum DriverError {
    CompilationFailed { message: &'static str },
    EntryFileMissing { path: PathBuf },
    SourceRootMissing { entry_file: PathBuf },
    ReadFile { path: PathBuf, source: io::Error },
    InvalidModulePath { file: PathBuf, root: PathBuf },
    EmptyModulePath { file: PathBuf },
    ExpectedSolveigFile { file: PathBuf },
    UnknownCompilerPrelude { module: String },
    DuplicateDefinition { kind: &'static str, name: String },
    Runtime { message: String },
    WriteOutput { source: io::Error },
    DumpBytecode { source: io::Error },
}

impl DriverError {
    pub(crate) fn compilation_failed(message: &'static str) -> Self {
        Self::CompilationFailed { message }
    }

    pub(crate) fn runtime(message: impl Into<String>) -> Self {
        Self::Runtime {
            message: message.into(),
        }
    }
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CompilationFailed { message } => write!(f, "{message}"),
            Self::EntryFileMissing { path } => {
                write!(f, "could not find entry file at {}", path.display())
            }
            Self::SourceRootMissing { entry_file } => {
                write!(
                    f,
                    "cannot determine source root for {}",
                    entry_file.display()
                )
            }
            Self::ReadFile { path, source } => {
                write!(f, "failed to read {}: {source}", path.display())
            }
            Self::InvalidModulePath { file, root } => {
                write!(f, "{} is not under {}", file.display(), root.display())
            }
            Self::EmptyModulePath { file } => write!(f, "empty module path {}", file.display()),
            Self::ExpectedSolveigFile { file } => {
                write!(f, "expected .sv file: {}", file.display())
            }
            Self::UnknownCompilerPrelude { module } => {
                write!(f, "unknown compiler prelude module {module}")
            }
            Self::DuplicateDefinition { kind, name } => write!(f, "duplicate {kind} {name}"),
            Self::Runtime { message } => write!(f, "runtime error: {message}"),
            Self::WriteOutput { source } => write!(f, "failed to write output: {source}"),
            Self::DumpBytecode { source } => write!(f, "failed to dump bytecode: {source}"),
        }
    }
}

impl std::error::Error for DriverError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ReadFile { source, .. }
            | Self::WriteOutput { source }
            | Self::DumpBytecode { source } => Some(source),
            _ => None,
        }
    }
}
