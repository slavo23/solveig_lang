//! Installed-library discovery for the Prelude and standard-library packages.
//!
//! Selection is deterministic: an explicit environment override wins, then a
//! `lib` directory beside the executable, then the caller's embedded fallback.

use camino::{Utf8Path, Utf8PathBuf};
use std::ffi::OsString;
use std::path::Path;

use super::{MANIFEST_FILE, Manifest, PackageError, SOURCE_DIRECTORY, canonical_utf8};

/// Names the environment override for the compiler-library root.
pub const LIBRARY_ENV_VAR: &str = "SOLVEIG_LIB";

/// Identifies which step of library discovery selected a location.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibraryOrigin {
    /// The `SOLVEIG_LIB` override selected the root.
    Environment,
    /// A `lib` directory beside the current executable selected the root.
    Executable,
    /// The compiler's embedded source copy selected the root.
    Embedded,
}

impl LibraryOrigin {
    /// Returns a stable description suitable for tracing output.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Environment => "environment",
            Self::Executable => "executable-relative",
            Self::Embedded => "embedded fallback",
        }
    }
}

/// Stores the selected compiler-library root and its discovery origin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibraryLocation {
    root: Utf8PathBuf,
    origin: LibraryOrigin,
}

impl LibraryLocation {
    /// Returns the absolute `lib` root.
    pub fn root(&self) -> &Utf8Path {
        &self.root
    }

    /// Returns the discovery step that selected the root.
    pub fn origin(&self) -> LibraryOrigin {
        self.origin
    }

    /// Returns the root of a named library package.
    pub fn package_root(&self, package: &str) -> Utf8PathBuf {
        self.root.join(package)
    }

    /// Returns the source root of a named library package.
    pub fn package_source_root(&self, package: &str) -> Utf8PathBuf {
        self.package_root(package).join(SOURCE_DIRECTORY)
    }
}

/// Resolves the compiler-library root using the locked discovery order.
pub fn resolve_library_location(embedded_root: &Utf8Path) -> Result<LibraryLocation, PackageError> {
    resolve_library_location_from(
        std::env::var_os(LIBRARY_ENV_VAR),
        std::env::current_exe().ok().as_deref(),
        embedded_root,
    )
}

fn resolve_library_location_from(
    environment: Option<OsString>,
    executable: Option<&Path>,
    embedded_root: &Utf8Path,
) -> Result<LibraryLocation, PackageError> {
    if let Some(environment) = environment {
        let path = Utf8PathBuf::from_path_buf(environment.into())
            .map_err(|path| PackageError::NonUtf8Path { path })?;
        return checked_location(&path, LibraryOrigin::Environment);
    }

    if let Some(executable) = executable
        && let Some(directory) = executable.parent()
    {
        let candidate = directory.join("lib");
        if is_library_root(&candidate) {
            let candidate = Utf8PathBuf::from_path_buf(candidate)
                .map_err(|path| PackageError::NonUtf8Path { path })?;
            return checked_location(&candidate, LibraryOrigin::Executable);
        }
    }

    if !embedded_root.is_absolute() {
        return Err(PackageError::InvalidLibraryRoot {
            path: embedded_root.to_path_buf(),
            message: "embedded fallback path must be absolute",
        });
    }

    if embedded_root.exists() {
        checked_location(embedded_root, LibraryOrigin::Embedded)
    } else {
        Ok(LibraryLocation {
            root: embedded_root.to_path_buf(),
            origin: LibraryOrigin::Embedded,
        })
    }
}

fn checked_location(
    root: &Utf8Path,
    origin: LibraryOrigin,
) -> Result<LibraryLocation, PackageError> {
    let root = canonical_utf8(root)?;
    if !is_library_root(root.as_std_path()) {
        return Err(PackageError::InvalidLibraryRoot {
            path: root,
            message: "expected prelude/solveig.toml and std/solveig.toml",
        });
    }
    for package in ["prelude", "std"] {
        let manifest = Manifest::read(&root.join(package))?;
        if manifest.package.name != package {
            return Err(PackageError::InvalidLibraryRoot {
                path: root,
                message: "library package name must match its directory",
            });
        }
    }
    Ok(LibraryLocation { root, origin })
}

fn is_library_root(root: &Path) -> bool {
    ["prelude", "std"]
        .iter()
        .all(|package| root.join(package).join(MANIFEST_FILE).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn utf8_path(directory: &TempDir) -> &Utf8Path {
        Utf8Path::from_path(directory.path()).expect("temporary test paths should be UTF-8")
    }

    fn write_library(root: &Utf8Path) {
        for package in ["prelude", "std"] {
            let package_root = root.join(package);
            fs::create_dir_all(&package_root).expect("test library package should be creatable");
            let manifest = format!(
                "[package]\nname = \"{package}\"\nversion = \"0.1.0\"\n\n[language]\nentry = \"{package}\"\n"
            );
            fs::write(package_root.join(MANIFEST_FILE), manifest)
                .expect("test package manifest should be writable");
        }
    }

    #[test]
    fn environment_override_wins_over_executable_library() {
        let directory = TempDir::new().expect("temporary directory should be creatable");
        let root = utf8_path(&directory);
        let environment = root.join("environment-lib");
        let executable_directory = root.join("bin");
        let executable_library = executable_directory.join("lib");
        let embedded = root.join("embedded-lib");
        write_library(&environment);
        write_library(&executable_library);
        fs::create_dir_all(&embedded).expect("embedded directory should be creatable");

        let location = resolve_library_location_from(
            Some(environment.as_os_str().to_owned()),
            Some(executable_directory.join("solveig").as_std_path()),
            &embedded,
        )
        .expect("environment library should resolve");

        assert_eq!(location.root(), environment.canonicalize_utf8().unwrap());
        assert_eq!(location.origin(), LibraryOrigin::Environment);
    }

    #[test]
    fn executable_library_precedes_embedded_fallback() {
        let directory = TempDir::new().expect("temporary directory should be creatable");
        let root = utf8_path(&directory);
        let executable_directory = root.join("bin");
        let executable_library = executable_directory.join("lib");
        let embedded = root.join("embedded-lib");
        write_library(&executable_library);
        fs::create_dir_all(&embedded).expect("embedded directory should be creatable");

        let location = resolve_library_location_from(
            None,
            Some(executable_directory.join("solveig").as_std_path()),
            &embedded,
        )
        .expect("executable library should resolve");

        assert_eq!(
            location.root(),
            executable_library.canonicalize_utf8().unwrap()
        );
        assert_eq!(location.origin(), LibraryOrigin::Executable);
    }

    #[test]
    fn embedded_library_is_the_last_resort() {
        let directory = TempDir::new().expect("temporary directory should be creatable");
        let root = utf8_path(&directory);
        let executable = root.join("bin/solveig");
        let embedded = root.join("embedded-lib");

        let location =
            resolve_library_location_from(None, Some(executable.as_std_path()), &embedded)
                .expect("embedded library should resolve");

        assert_eq!(location.root(), embedded);
        assert_eq!(location.origin(), LibraryOrigin::Embedded);
    }
}
