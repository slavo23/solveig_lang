//! Solveig package manifests, source layout, and path-dependency graphs.
//!
//! This crate deliberately does not parse Solveig source. It establishes which
//! package and source files the compiler should load.
//!
//! A package has a strict `solveig.toml` manifest. The currently supported
//! language configuration is intentionally small: `language.entry` is a
//! dot-separated module name resolved beneath the package's `src/` directory.
//! Dependencies are local paths; registry and version resolution are outside
//! this crate's scope.

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use solveig_base::FxHashMap;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

mod library;

pub use library::{LIBRARY_ENV_VAR, LibraryLocation, LibraryOrigin, resolve_library_location};

pub const MANIFEST_FILE: &str = "solveig.toml";
pub const SOURCE_DIRECTORY: &str = "src";
pub const SOURCE_EXTENSION: &str = "sv";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub package: PackageManifest,
    #[serde(default)]
    pub dependencies: BTreeMap<String, PathDependency>,
    pub language: LanguageManifest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathDependency {
    pub path: Utf8PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageManifest {
    pub entry: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implicit: Vec<String>,
}

impl Manifest {
    pub fn parse(source: &str) -> Result<Self, PackageError> {
        let manifest: Self = toml::from_str(source).map_err(PackageError::ParseManifest)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn read(package_root: &Utf8Path) -> Result<Self, PackageError> {
        let path = package_root.join(MANIFEST_FILE);
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                return Err(PackageError::MissingManifest { path });
            }
            Err(source) => return Err(PackageError::ReadManifest { path, source }),
        };

        Self::parse(&source).map_err(|error| error.with_manifest_path(path))
    }

    pub fn to_toml(&self) -> Result<String, PackageError> {
        self.validate()?;
        toml::to_string_pretty(self).map_err(PackageError::SerializeManifest)
    }

    pub fn source_root(&self, package_root: &Utf8Path) -> Utf8PathBuf {
        package_root.join(SOURCE_DIRECTORY)
    }

    pub fn entry_file(&self, package_root: &Utf8Path) -> Utf8PathBuf {
        let mut path = self.source_root(package_root);
        for segment in self.language.entry.split('.') {
            path.push(segment);
        }
        path.set_extension(SOURCE_EXTENSION);
        path
    }

    fn validate(&self) -> Result<(), PackageError> {
        validate_module_segment("package.name", &self.package.name)?;
        validate_nonempty("package.version", &self.package.version)?;
        validate_module_name("language.entry", &self.language.entry)?;
        for module in &self.language.implicit {
            validate_module_name("language.implicit", module)?;
        }

        for (name, dependency) in &self.dependencies {
            validate_nonempty("dependency name", name)?;
            if dependency.path.as_str().is_empty() {
                return Err(PackageError::InvalidField {
                    field: "dependencies.*.path",
                    message: "path must not be empty".to_string(),
                });
            }
        }

        Ok(())
    }
}

fn validate_module_segment(field: &'static str, value: &str) -> Result<(), PackageError> {
    validate_nonempty(field, value)?;
    let mut chars = value.chars();
    let starts_valid = chars
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_');
    if !starts_valid
        || !chars
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '\''))
    {
        return Err(PackageError::InvalidField {
            field,
            message: "value must be a valid Solveig module segment".to_string(),
        });
    }
    Ok(())
}

fn validate_nonempty(field: &'static str, value: &str) -> Result<(), PackageError> {
    if value.trim().is_empty() {
        Err(PackageError::InvalidField {
            field,
            message: "value must not be empty".to_string(),
        })
    } else {
        Ok(())
    }
}

fn validate_module_name(field: &'static str, value: &str) -> Result<(), PackageError> {
    validate_nonempty(field, value)?;
    if value
        .split('.')
        .any(|segment| segment.is_empty() || segment.contains(['/', '\\']))
    {
        return Err(PackageError::InvalidField {
            field,
            message: "module segments must be non-empty file names".to_string(),
        });
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PackageId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackageKind {
    Local,
    Implicit,
    Dependency,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Package {
    pub id: PackageId,
    pub name: String,
    pub version: String,
    pub root: Utf8PathBuf,
    pub kind: PackageKind,
    pub modules: Vec<Utf8PathBuf>,
    pub deps: BTreeMap<String, PackageId>,
    pub language: LanguageManifest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageGraph {
    pub root: PackageId,
    packages: Vec<Package>,
}

impl PackageGraph {
    pub fn load(root: &Utf8Path) -> Result<Self, PackageError> {
        let root = canonical_utf8(root)?;
        let mut loader = GraphLoader::default();
        let root_id = loader.load(&root, PackageKind::Local)?;
        validate_unique_package_names(&loader.packages)?;
        Ok(Self {
            root: root_id,
            packages: loader.packages,
        })
    }

    pub fn packages(&self) -> &[Package] {
        &self.packages
    }

    pub fn package(&self, id: PackageId) -> Option<&Package> {
        usize::try_from(id.0)
            .ok()
            .and_then(|index| self.packages.get(index))
    }
}

fn validate_unique_package_names(packages: &[Package]) -> Result<(), PackageError> {
    let mut roots = BTreeMap::<&str, &Utf8Path>::new();
    for package in packages {
        if let Some(first) = roots.insert(&package.name, &package.root) {
            return Err(PackageError::PackageNameCollision {
                name: package.name.clone(),
                first: first.to_path_buf(),
                second: package.root.clone(),
            });
        }
    }
    Ok(())
}

#[derive(Default)]
struct GraphLoader {
    packages: Vec<Package>,
    loaded: FxHashMap<Utf8PathBuf, PackageId>,
    stack: Vec<Utf8PathBuf>,
}

impl GraphLoader {
    fn load(&mut self, root: &Utf8Path, kind: PackageKind) -> Result<PackageId, PackageError> {
        if let Some(id) = self.loaded.get(root) {
            return Ok(*id);
        }
        if let Some(cycle_start) = self.stack.iter().position(|candidate| candidate == root) {
            let mut cycle: Vec<_> = self.stack.iter().skip(cycle_start).cloned().collect();
            cycle.push(root.to_path_buf());
            return Err(PackageError::DependencyCycle { cycle });
        }

        self.stack.push(root.to_path_buf());
        let result = self.load_new(root, kind);
        let _popped = self.stack.pop();
        result
    }

    fn load_new(&mut self, root: &Utf8Path, kind: PackageKind) -> Result<PackageId, PackageError> {
        let manifest = Manifest::read(root)?;
        let mut deps = BTreeMap::new();
        for (name, dependency) in &manifest.dependencies {
            let dependency_root = canonical_utf8(&root.join(&dependency.path))?;
            let id = self.load(&dependency_root, PackageKind::Dependency)?;
            deps.insert(name.clone(), id);
        }

        let raw_id = u32::try_from(self.packages.len()).map_err(|_source| {
            PackageError::TooManyPackages {
                count: self.packages.len(),
            }
        })?;
        let id = PackageId(raw_id);
        let modules = discover_modules(&manifest.source_root(root))?;
        self.packages.push(Package {
            id,
            name: manifest.package.name,
            version: manifest.package.version,
            root: root.to_path_buf(),
            kind,
            modules,
            deps,
            language: manifest.language,
        });
        self.loaded.insert(root.to_path_buf(), id);
        Ok(id)
    }
}

pub fn find_nearest_manifest(source_file: &Utf8Path) -> Option<Utf8PathBuf> {
    let source_directory = source_file.parent()?;
    source_directory
        .ancestors()
        .find(|candidate| is_package_root(candidate.as_std_path()))
        .map(Utf8Path::to_path_buf)
}

pub fn find_nearest_manifest_path(source_file: &Path) -> Option<PathBuf> {
    Utf8Path::from_path(source_file)
        .and_then(find_nearest_manifest)
        .map(Utf8PathBuf::into_std_path_buf)
}

pub fn is_package_root(path: &Path) -> bool {
    path.join(MANIFEST_FILE).is_file()
}

fn canonical_utf8(path: &Utf8Path) -> Result<Utf8PathBuf, PackageError> {
    let canonical = fs::canonicalize(path).map_err(|source| PackageError::CanonicalizePath {
        path: path.to_path_buf(),
        source,
    })?;
    Utf8PathBuf::from_path_buf(canonical).map_err(|path| PackageError::NonUtf8Path { path })
}

fn discover_modules(source_root: &Utf8Path) -> Result<Vec<Utf8PathBuf>, PackageError> {
    let mut modules = Vec::new();
    if !source_root.exists() {
        return Ok(modules);
    }
    discover_modules_in(source_root, &mut modules)?;
    modules.sort();
    Ok(modules)
}

fn discover_modules_in(
    directory: &Utf8Path,
    modules: &mut Vec<Utf8PathBuf>,
) -> Result<(), PackageError> {
    let entries = fs::read_dir(directory).map_err(|source| PackageError::ReadSourceDirectory {
        path: directory.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| PackageError::ReadSourceDirectory {
            path: directory.to_path_buf(),
            source,
        })?;
        let file_type = entry
            .file_type()
            .map_err(|source| PackageError::ReadSourceDirectory {
                path: directory.to_path_buf(),
                source,
            })?;
        let path = Utf8PathBuf::from_path_buf(entry.path())
            .map_err(|path| PackageError::NonUtf8Path { path })?;
        if file_type.is_dir() {
            discover_modules_in(&path, modules)?;
        } else if file_type.is_file() && path.extension() == Some(SOURCE_EXTENSION) {
            modules.push(path);
        }
    }
    Ok(())
}

#[derive(Debug)]
pub enum PackageError {
    MissingManifest {
        path: Utf8PathBuf,
    },
    ReadManifest {
        path: Utf8PathBuf,
        source: io::Error,
    },
    ParseManifest(toml::de::Error),
    ParseManifestAt {
        path: Utf8PathBuf,
        source: toml::de::Error,
    },
    SerializeManifest(toml::ser::Error),
    InvalidField {
        field: &'static str,
        message: String,
    },
    CanonicalizePath {
        path: Utf8PathBuf,
        source: io::Error,
    },
    NonUtf8Path {
        path: PathBuf,
    },
    ReadSourceDirectory {
        path: Utf8PathBuf,
        source: io::Error,
    },
    DependencyCycle {
        cycle: Vec<Utf8PathBuf>,
    },
    PackageNameCollision {
        name: String,
        first: Utf8PathBuf,
        second: Utf8PathBuf,
    },
    TooManyPackages {
        count: usize,
    },
    InvalidLibraryRoot {
        path: Utf8PathBuf,
        message: &'static str,
    },
}

impl PackageError {
    fn with_manifest_path(self, path: Utf8PathBuf) -> Self {
        match self {
            Self::ParseManifest(source) => Self::ParseManifestAt { path, source },
            other => other,
        }
    }
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingManifest { path } => write!(f, "missing package manifest {path}"),
            Self::ReadManifest { path, source } => {
                write!(f, "failed to read package manifest {path}: {source}")
            }
            Self::ParseManifest(source) => write!(f, "invalid package manifest: {source}"),
            Self::ParseManifestAt { path, source } => {
                write!(f, "invalid package manifest {path}: {source}")
            }
            Self::SerializeManifest(source) => {
                write!(f, "failed to serialize package manifest: {source}")
            }
            Self::InvalidField { field, message } => {
                write!(f, "invalid manifest field {field}: {message}")
            }
            Self::CanonicalizePath { path, source } => {
                write!(f, "failed to resolve package path {path}: {source}")
            }
            Self::NonUtf8Path { path } => {
                write!(f, "package path is not valid UTF-8: {}", path.display())
            }
            Self::ReadSourceDirectory { path, source } => {
                write!(f, "failed to read source directory {path}: {source}")
            }
            Self::DependencyCycle { cycle } => {
                let rendered = cycle
                    .iter()
                    .map(|path| path.as_str())
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(f, "cyclic path dependency: {rendered}")
            }
            Self::PackageNameCollision {
                name,
                first,
                second,
            } => write!(
                f,
                "package name `{name}` is claimed by both {first} and {second}"
            ),
            Self::TooManyPackages { count } => {
                write!(f, "package graph has too many packages ({count})")
            }
            Self::InvalidLibraryRoot { path, message } => {
                write!(f, "invalid Solveig library root {path}: {message}")
            }
        }
    }
}

impl std::error::Error for PackageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ReadManifest { source, .. }
            | Self::CanonicalizePath { source, .. }
            | Self::ReadSourceDirectory { source, .. } => Some(source),
            Self::ParseManifest(source) | Self::ParseManifestAt { source, .. } => Some(source),
            Self::SerializeManifest(source) => Some(source),
            Self::MissingManifest { .. }
            | Self::InvalidField { .. }
            | Self::NonUtf8Path { .. }
            | Self::DependencyCycle { .. }
            | Self::PackageNameCollision { .. }
            | Self::TooManyPackages { .. }
            | Self::InvalidLibraryRoot { .. } => None,
        }
    }
}
