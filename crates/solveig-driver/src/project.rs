//! Project discovery and file loading for Solveig packages.
//! The compile facade calls this before parsing modules into the project graph.

use solveig_base::FxHashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use solveig_base::{FileId, FileSpan, Session};
use solveig_package::{LibraryLocation, LibraryOrigin, find_nearest_manifest};
use solveig_resolve::project::{
    ModuleFile, ModuleTable, ParsedProject, Project, ResolveError, effective_imports,
};
use solveig_syntax::ast::{ModuleName, ParsedModule};
use solveig_syntax::lowering;

use crate::DriverError;
use crate::library::{compiler_library_location, embedded_prelude_source};
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

fn prelude_module(session: &Session, module: &ModuleName) -> Option<&'static str> {
    embedded_prelude_source(&module.as_string(session))
}

pub fn is_compiler_prelude_module(session: &Session, module: &ModuleName) -> bool {
    prelude_module(session, module).is_some()
}

fn parse_source(session: &Session, file: FileId, source: &str) -> ParsedModule {
    lowering::parse_source(session, file, source)
}

fn source_hash(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

fn load_prelude_file(
    session: &Session,
    module: &ModuleName,
    sources: &SourceProvider,
    library: &LibraryLocation,
) -> Result<ModuleFile, ProjectError> {
    let root = library.package_source_root("prelude").into_std_path_buf();
    let path = file_from_module_name(session, &root, module);

    if library.origin() != LibraryOrigin::Embedded {
        return scan_module_file(session, &root, &path, sources).map_err(ProjectError::from);
    }

    let source = prelude_module(session, module)
        .ok_or_else(|| DriverError::UnknownCompilerPrelude {
            module: module.as_string(session),
        })?
        .to_string();
    Ok(module_file_from_source(
        session,
        module.clone(),
        path,
        &source,
    ))
}

fn load_prelude_module(
    session: &Session,
    modules: &mut FxHashMap<ModuleName, ModuleFile>,
    module: &ModuleName,
    sources: &SourceProvider,
    library: &LibraryLocation,
    cached_prelude_modules: Option<&FxHashMap<ModuleName, ModuleFile>>,
) -> Result<(), ProjectError> {
    if modules.contains_key(module) {
        return Ok(());
    }

    let module_file = cached_prelude_modules
        .and_then(|cached| cached.get(module).cloned())
        .map_or_else(|| load_prelude_file(session, module, sources, library), Ok)?;
    let imports = module_file.imports.clone();
    let file = module_file.parsed.file;
    modules.insert(module.clone(), module_file);

    for imp in effective_imports(session, module, &imports, file) {
        if prelude_module(session, &imp.module).is_some() {
            load_prelude_module(
                session,
                modules,
                &imp.module,
                sources,
                library,
                cached_prelude_modules,
            )?;
        } else {
            return Err(ResolveError::UnknownModule {
                span: imp.span,
                module: imp.module,
            }
            .into());
        }
    }

    Ok(())
}

fn scan_module_file(
    session: &Session,
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
        name,
        file.to_path_buf(),
        &source,
    ))
}

fn module_file_from_source(
    session: &Session,
    name: ModuleName,
    path: PathBuf,
    source: &str,
) -> ModuleFile {
    let file_id = session.add_or_update_file(path.clone(), source.to_string());
    let parsed = parse_source(session, file_id, source);
    let imports = parsed.imports.clone();

    ModuleFile {
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
    cached_prelude_modules: Option<&FxHashMap<ModuleName, ModuleFile>>,
) -> Result<(PathBuf, Project), ProjectError> {
    let library = compiler_library_location()?;
    let root = if let Some(entry_file) = camino::Utf8Path::from_path(entry_file)
        && let Some(package_root) = find_nearest_manifest(entry_file)
    {
        package_root
            .join(solveig_package::SOURCE_DIRECTORY)
            .into_std_path_buf()
    } else {
        entry_file
            .parent()
            .ok_or_else(|| DriverError::SourceRootMissing {
                entry_file: entry_file.to_path_buf(),
            })?
            .to_path_buf()
    };

    let mut modules = FxHashMap::<ModuleName, ModuleFile>::default();
    let mut stack = vec![PendingModule {
        file: entry_file.to_path_buf(),
        requested_module: None,
        import_span: None,
    }];

    while let Some(pending) = stack.pop() {
        let module_file = match scan_module_file(session, &root, &pending.file, sources) {
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
            for imp in effective_imports(
                session,
                &module_name,
                &module_file.imports,
                module_file.parsed.file,
            ) {
                if prelude_module(session, &imp.module).is_some() {
                    load_prelude_module(
                        session,
                        &mut modules,
                        &imp.module,
                        sources,
                        &library,
                        cached_prelude_modules,
                    )?;
                } else {
                    stack.push(PendingModule {
                        file: file_from_module_name(session, &root, &imp.module),
                        requested_module: Some(imp.module),
                        import_span: Some(imp.span),
                    });
                }
            }
        }

        modules.insert(module_name, module_file);
    }

    let module_table = ModuleTable::new(session, modules.keys());

    Ok((
        root,
        Project {
            modules,
            module_table,
        },
    ))
}
