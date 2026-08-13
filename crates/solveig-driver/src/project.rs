//! Project discovery and file loading for Solveig packages.
//! The compile facade calls this before parsing modules into the project graph.

use solveig_base::{FxHashMap, FxHashSet};
use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use solveig_base::{FileId, FileSpan, Session};
use solveig_package::{
    LibraryLocation, LibraryOrigin, Manifest, Package, PackageError, PackageGraph, PackageId,
    PackageKind, find_nearest_manifest,
};
use solveig_resolve::project::{ModuleFile, ParsedProject, Project, ResolveError};
use solveig_syntax::ast::{ModuleName, ParsedModule};
use solveig_syntax::lowering;

use crate::DriverError;
use crate::library::{
    compiler_library_location, embedded_library_manifest, embedded_library_modules,
    embedded_library_source,
};
use crate::source_provider::SourceProvider;

#[derive(Debug)]
pub enum ProjectError {
    Driver(DriverError),
    Resolve(ResolveError),
}

impl From<DriverError> for ProjectError {
    fn from(error: DriverError) -> Self {
        Self::Driver(error)
    }
}

impl From<ResolveError> for ProjectError {
    fn from(error: ResolveError) -> Self {
        Self::Resolve(error)
    }
}

impl From<PackageError> for ProjectError {
    fn from(error: PackageError) -> Self {
        Self::Driver(error.into())
    }
}

fn parse_source(session: &Session, file: FileId, source: &str) -> ParsedModule {
    lowering::parse_source(session, file, source)
}

fn source_hash(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

fn scan_module_file(
    session: &Session,
    package: PackageId,
    root: &Path,
    file: &Path,
    sources: &SourceProvider,
) -> Result<ModuleFile, DriverError> {
    let source = sources
        .read_to_string(file)
        .map_err(|source| DriverError::ReadFile {
            path: file.to_path_buf(),
            source,
        })?;

    let name = module_name_from_file(session, root, file)?;
    Ok(module_file_from_source(
        session,
        package,
        name,
        file.to_path_buf(),
        &source,
    ))
}

fn module_file_from_source(
    session: &Session,
    package: PackageId,
    name: ModuleName,
    path: PathBuf,
    source: &str,
) -> ModuleFile {
    let file_id = session.add_or_update_file(path.clone(), source.to_string());
    let parsed = parse_source(session, file_id, source);
    let imports = parsed.imports.clone();

    ModuleFile {
        package,
        name,
        path,
        source_hash: source_hash(source),
        imports,
        parsed,
    }
}

pub fn module_name_from_file(
    session: &Session,
    root: &Path,
    file: &Path,
) -> Result<ModuleName, DriverError> {
    let rel = file
        .strip_prefix(root)
        .map_err(|_error| DriverError::InvalidModulePath {
            file: file.to_path_buf(),
            root: root.to_path_buf(),
        })?;

    let mut parts = rel
        .iter()
        .map(|s| s.to_string_lossy().to_string())
        .collect::<Vec<_>>();

    let last = parts
        .last_mut()
        .ok_or_else(|| DriverError::EmptyModulePath {
            file: file.to_path_buf(),
        })?;
    if let Some(stem) = last.strip_suffix(".sv") {
        *last = stem.to_string();
    } else {
        return Err(DriverError::ExpectedSolveigFile {
            file: file.to_path_buf(),
        });
    }

    Ok(ModuleName(
        parts.iter().map(|part| session.intern(part)).collect(),
    ))
}

pub fn file_from_module_name(session: &Session, root: &Path, module: &ModuleName) -> PathBuf {
    let mut out = root.to_path_buf();
    for seg in &module.0 {
        out.push(&*session.str(*seg));
    }
    out.set_extension("sv");
    out
}

pub fn parse_project(project: &Project) -> ParsedProject {
    let mut modules = FxHashMap::default();

    for (name, file) in &project.modules {
        modules.insert(name.clone(), file.parsed.clone());
    }

    ParsedProject { modules }
}

struct PendingModule {
    file: PathBuf,
    requested_module: Option<ModuleName>,
    import_span: Option<FileSpan>,
}

pub(crate) fn load_project_with_source_provider(
    session: &Session,
    entry_file: &Path,
    sources: &SourceProvider,
) -> Result<(PathBuf, Project), ProjectError> {
    let library = compiler_library_location()?;
    if let Some(entry_file) = camino::Utf8Path::from_path(entry_file)
        && let Some(package_root) = find_nearest_manifest(entry_file)
    {
        return load_package_project(
            session,
            &package_root,
            entry_file.as_std_path(),
            sources,
            &library,
        );
    }

    load_legacy_project(session, entry_file, sources, &library)
}

fn load_legacy_project(
    session: &Session,
    entry_file: &Path,
    sources: &SourceProvider,
    library: &LibraryLocation,
) -> Result<(PathBuf, Project), ProjectError> {
    let root = entry_file
        .parent()
        .ok_or_else(|| DriverError::SourceRootMissing {
            entry_file: entry_file.to_path_buf(),
        })?
        .to_path_buf();
    let local_package = PackageId(0);
    let prelude_package = PackageId(1);
    let entry_module = module_name_from_file(session, &root, entry_file)?;

    let mut modules = FxHashMap::<ModuleName, ModuleFile>::default();
    let mut compiler_packages = Vec::new();
    let temporary_id = add_compiler_package(
        &mut compiler_packages,
        library,
        "prelude",
        PackageKind::Implicit,
    )?;
    let mut prelude = compiler_packages
        .pop()
        .unwrap_or_else(|| solveig_base::ice!("compiler Prelude package was not added"));
    if prelude.id != temporary_id {
        solveig_base::ice!("compiler Prelude package received an unexpected identity");
    }
    prelude.id = prelude_package;
    let prelude_source_root = prelude.root.join(solveig_package::SOURCE_DIRECTORY);
    for file in &prelude.modules {
        let module = scan_package_module(
            session,
            &prelude,
            &prelude_source_root,
            file,
            sources,
            library,
        )?;
        modules.insert(module.name.clone(), module);
    }
    let implicit_modules = prelude
        .language
        .implicit
        .iter()
        .map(|module| module_name_from_text(session, module))
        .collect::<Vec<_>>();
    let mut stack = vec![PendingModule {
        file: entry_file.to_path_buf(),
        requested_module: None,
        import_span: None,
    }];

    while let Some(pending) = stack.pop() {
        let module_file =
            match scan_module_file(session, local_package, &root, &pending.file, sources) {
                Ok(module_file) => module_file,
                Err(error) => {
                    if !sources.has_source(&pending.file)
                        && let (Some(module), Some(span)) =
                            (pending.requested_module, pending.import_span)
                    {
                        return Err(ResolveError::UnknownModule { span, module }.into());
                    }

                    return Err(ProjectError::Driver(error));
                }
            };
        if modules.contains_key(&module_file.name) {
            continue;
        }

        let module_name = module_file.name.clone();

        if !session.diagnostics().has_errors() {
            for imp in &module_file.imports {
                if modules.contains_key(&imp.module) {
                    continue;
                }
                stack.push(PendingModule {
                    file: file_from_module_name(session, &root, &imp.module),
                    requested_module: Some(imp.module.clone()),
                    import_span: Some(FileSpan::new(module_file.parsed.file, imp.span)),
                });
            }
        }

        modules.insert(module_name, module_file);
    }

    let package_kinds = FxHashMap::from_iter([
        (local_package, PackageKind::Local),
        (prelude_package, PackageKind::Implicit),
    ]);
    let project = Project::new(
        session,
        modules,
        package_kinds,
        implicit_modules,
        entry_module,
    );

    Ok((root, project))
}

fn load_package_project(
    session: &Session,
    package_root: &camino::Utf8Path,
    entry_file: &Path,
    sources: &SourceProvider,
    library: &LibraryLocation,
) -> Result<(PathBuf, Project), ProjectError> {
    let graph = PackageGraph::load(package_root)?;
    let mut packages = graph.packages().to_vec();
    if let Some(local) = packages.iter_mut().find(|package| package.id == graph.root) {
        let canonical_root = local.root.clone();
        if canonical_root != package_root {
            local.modules = local
                .modules
                .iter()
                .map(|module| {
                    module
                        .strip_prefix(&canonical_root)
                        .map_or_else(|_| module.clone(), |relative| package_root.join(relative))
                })
                .collect();
            local.root = package_root.to_path_buf();
        }
        if !local
            .modules
            .iter()
            .any(|module| module.as_std_path() == entry_file)
        {
            local.modules.push(
                camino::Utf8PathBuf::from_path_buf(entry_file.to_path_buf())
                    .map_err(|path| PackageError::NonUtf8Path { path })?,
            );
            local.modules.sort();
        }
    }

    let prelude_package =
        add_compiler_package(&mut packages, library, "prelude", PackageKind::Implicit)?;
    let mut modules = FxHashMap::<ModuleName, ModuleFile>::default();
    for package in &packages {
        let source_root = package.root.join(solveig_package::SOURCE_DIRECTORY);
        for file in &package.modules {
            let module =
                scan_package_module(session, package, &source_root, file, sources, library)?;
            let module_name = module.name.clone();
            if modules.insert(module_name.clone(), module).is_some() {
                return Err(DriverError::DuplicateDefinition {
                    kind: "module",
                    name: module_name.as_string(session),
                }
                .into());
            }
        }
    }

    qualify_package_imports(session, &packages, &mut modules);
    let package_kinds = packages
        .iter()
        .map(|package| (package.id, package.kind))
        .collect();
    let implicit_modules = packages
        .iter()
        .find(|package| package.id == prelude_package)
        .map(|package| {
            package
                .language
                .implicit
                .iter()
                .map(|module| module_name_from_text(session, module))
                .filter(|module| modules.contains_key(module))
                .collect()
        })
        .unwrap_or_default();
    let local_package = packages
        .iter()
        .find(|package| package.id == graph.root)
        .unwrap_or_else(|| solveig_base::ice!("package graph lost its root package"));
    let source_root = local_package
        .root
        .join(solveig_package::SOURCE_DIRECTORY)
        .into_std_path_buf();
    let entry_module = package_module_name(
        session,
        &local_package.name,
        module_name_from_text(session, &local_package.language.entry),
    );
    let project = Project::new(
        session,
        modules,
        package_kinds,
        implicit_modules,
        entry_module,
    );

    Ok((source_root, project))
}

fn add_compiler_package(
    packages: &mut Vec<Package>,
    library: &LibraryLocation,
    name: &'static str,
    kind: PackageKind,
) -> Result<PackageId, ProjectError> {
    let root = library.package_root(name);
    if let Some(package) = packages.iter_mut().find(|package| package.root == root) {
        package.kind = kind;
        return Ok(package.id);
    }
    if let Some(package) = packages.iter().find(|package| package.name == name) {
        return Err(PackageError::PackageNameCollision {
            name: name.to_string(),
            first: package.root.clone(),
            second: root,
        }
        .into());
    }

    let id = PackageId(u32::try_from(packages.len()).map_err(|_source| {
        PackageError::TooManyPackages {
            count: packages.len(),
        }
    })?);
    let package = if root.exists() {
        let graph = PackageGraph::load(&root)?;
        let mut package = graph
            .package(graph.root)
            .cloned()
            .unwrap_or_else(|| solveig_base::ice!("compiler package graph lost its root"));
        package.id = id;
        package.kind = kind;
        package
    } else {
        let manifest = compiler_manifest(library, name)?;
        Package {
            id,
            name: manifest.package.name,
            version: manifest.package.version,
            root: root.clone(),
            kind,
            modules: embedded_library_modules(name)
                .iter()
                .map(|relative| root.join(solveig_package::SOURCE_DIRECTORY).join(relative))
                .collect(),
            deps: BTreeMap::new(),
            language: manifest.language,
        }
    };
    packages.push(package);
    Ok(id)
}

fn scan_package_module(
    session: &Session,
    package: &Package,
    source_root: &camino::Utf8Path,
    file: &camino::Utf8Path,
    sources: &SourceProvider,
    library: &LibraryLocation,
) -> Result<ModuleFile, ProjectError> {
    let raw_name = module_name_from_file(session, source_root.as_std_path(), file.as_std_path())?;
    let name = package_module_name(session, &package.name, raw_name);
    let source = match sources.read_to_string(file.as_std_path()) {
        Ok(source) => source,
        Err(source)
            if library.origin() == LibraryOrigin::Embedded
                && matches!(
                    package.kind,
                    PackageKind::Implicit | PackageKind::Dependency
                ) =>
        {
            embedded_library_source(&name.as_string(session))
                .ok_or_else(|| DriverError::ReadFile {
                    path: file.as_std_path().to_path_buf(),
                    source,
                })?
                .to_string()
        }
        Err(source) => {
            return Err(DriverError::ReadFile {
                path: file.as_std_path().to_path_buf(),
                source,
            }
            .into());
        }
    };

    Ok(module_file_from_source(
        session,
        package.id,
        name,
        file.as_std_path().to_path_buf(),
        &source,
    ))
}

fn package_module_name(session: &Session, package: &str, mut module: ModuleName) -> ModuleName {
    let package = session.intern(package);
    if module.0.first().copied() != Some(package) {
        module.0.insert(0, package);
    }
    module
}

fn qualify_package_imports(
    session: &Session,
    packages: &[Package],
    modules: &mut FxHashMap<ModuleName, ModuleFile>,
) {
    let module_names = modules.keys().cloned().collect::<FxHashSet<_>>();
    let package_names = packages
        .iter()
        .map(|package| (package.id, session.intern(&package.name)))
        .collect::<FxHashMap<_, _>>();
    let package_prefixes = package_names.values().copied().collect::<FxHashSet<_>>();

    for module in modules.values_mut() {
        let Some(package_name) = package_names.get(&module.package).copied() else {
            continue;
        };
        for import in &mut module.parsed.imports {
            let mut relative = import.module.clone();
            relative.0.insert(0, package_name);
            let starts_with_package = import
                .module
                .0
                .first()
                .is_some_and(|segment| package_prefixes.contains(segment));
            if module_names.contains(&relative) || !starts_with_package {
                import.module = relative;
            }
        }
        module.imports = module.parsed.imports.clone();
    }
}

fn module_name_from_text(session: &Session, name: &str) -> ModuleName {
    ModuleName(
        name.split('.')
            .map(|segment| session.intern(segment))
            .collect(),
    )
}

fn compiler_manifest(
    library: &LibraryLocation,
    package: &'static str,
) -> Result<Manifest, ProjectError> {
    let root = library.package_root(package);
    if root.exists() {
        return Manifest::read(&root).map_err(ProjectError::from);
    }

    let source =
        embedded_library_manifest(package).ok_or_else(|| PackageError::InvalidLibraryRoot {
            path: library.root().to_path_buf(),
            message: "embedded package manifest is missing",
        })?;
    Manifest::parse(source).map_err(ProjectError::from)
}
