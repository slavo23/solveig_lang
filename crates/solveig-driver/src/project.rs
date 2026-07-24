use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use solveig_base::{FileId, FileSpan, Session};
use solveig_resolve::project::{
    ModuleFile, ModuleTable, ParsedProject, Project, ResolveError, effective_imports,
};
use solveig_syntax::ast::{ModuleName, ParsedModule};
use solveig_syntax::parser;

const PRELUDE_BUILTINS_SRC: &str = include_str!("./prelude/builtins.sv");
const PRELUDE_LIST_SRC: &str = include_str!("./prelude/list.sv");
const PRELUDE_TYPECLASS_SRC: &str = include_str!("./prelude/typeclasses.sv");
const PRELUDE_PRIM_SRC: &str = "";

#[derive(Debug, Clone)]
pub enum ProjectError {
    Message(String),
    Resolve(ResolveError),
}

impl From<String> for ProjectError {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl From<ResolveError> for ProjectError {
    fn from(error: ResolveError) -> Self {
        Self::Resolve(error)
    }
}

fn prelude_module(session: &Session, module: &ModuleName) -> Option<&'static str> {
    match module.as_string(session).as_str() {
        "prelude.builtins" => Some(PRELUDE_BUILTINS_SRC),
        "prelude.list" => Some(PRELUDE_LIST_SRC),
        "prelude.typeclasses" => Some(PRELUDE_TYPECLASS_SRC),
        "prelude.prim" => Some(PRELUDE_PRIM_SRC),
        _ => None,
    }
}

fn parse_source(session: &Session, file: FileId, source: &str) -> ParsedModule {
    parser::parse_source(session, file, source)
}

fn mk_virtual_prelude_file(
    session: &Session,
    module: &ModuleName,
) -> Result<ModuleFile, ProjectError> {
    let source = prelude_module(session, module)
        .ok_or_else(|| {
            format!(
                "unknown compiler prelude module {}",
                module.as_string(session)
            )
        })?
        .to_string();

    let path = PathBuf::from(format!(
        "<compiler-prelude>/{}.sv",
        module.as_string(session)
    ));
    let file = session.add_file(path.clone(), source.clone());
    let parsed = parse_source(session, file, &source);
    let imports = parsed.imports.clone();

    Ok(ModuleFile {
        name: module.clone(),
        path,
        imports,
        parsed,
    })
}

fn scan_module_file(session: &Session, root: &Path, file: &Path) -> Result<ModuleFile, String> {
    let source = fs::read_to_string(file)
        .map_err(|e| format!("failed to read {}: {}", file.display(), e))?;

    let name = module_name_from_file(session, root, file)?;
    let file_id = session.add_file(file.to_path_buf(), source.clone());
    let parsed = parse_source(session, file_id, &source);
    let imports = parsed.imports.clone();

    Ok(ModuleFile {
        name,
        path: file.to_path_buf(),
        imports,
        parsed,
    })
}

pub fn module_name_from_file(
    session: &Session,
    root: &Path,
    file: &Path,
) -> Result<ModuleName, String> {
    let rel = file
        .strip_prefix(root)
        .map_err(|_| format!("{} is not under {}", file.display(), root.display()))?;

    let mut parts = rel
        .iter()
        .map(|s| s.to_string_lossy().to_string())
        .collect::<Vec<_>>();

    let last = parts.last_mut().ok_or_else(|| "empty path".to_string())?;
    if let Some(stem) = last.strip_suffix(".sv") {
        *last = stem.to_string();
    } else {
        return Err(format!("expected .sv file: {}", file.display()));
    }

    Ok(ModuleName(
        parts.iter().map(|part| session.intern(part)).collect(),
    ))
}

pub fn file_from_module_name(session: &Session, root: &Path, module: &ModuleName) -> PathBuf {
    let mut out = root.to_path_buf();
    for seg in &module.0 {
        out.push(session.str(*seg));
    }
    out.set_extension("sv");
    out
}

pub fn parse_project(project: &Project) -> ParsedProject {
    let mut modules = HashMap::new();

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

pub fn load_project(
    session: &Session,
    entry_file: &Path,
) -> Result<(PathBuf, Project), ProjectError> {
    let root = entry_file
        .parent()
        .ok_or_else(|| format!("cannot determine source root for {}", entry_file.display()))?
        .to_path_buf();

    let mut modules = HashMap::<ModuleName, ModuleFile>::new();
    let mut stack = vec![PendingModule {
        file: entry_file.to_path_buf(),
        requested_module: None,
        import_span: None,
    }];

    while let Some(pending) = stack.pop() {
        let module_file = match scan_module_file(session, &root, &pending.file) {
            Ok(module_file) => module_file,
            Err(message) => {
                if !pending.file.is_file()
                    && let (Some(module), Some(span)) =
                        (pending.requested_module, pending.import_span)
                {
                    return Err(ResolveError::UnknownModule { span, module }.into());
                }

                return Err(ProjectError::Message(message));
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
                    if !modules.contains_key(&imp.module) {
                        modules.insert(
                            imp.module.clone(),
                            mk_virtual_prelude_file(session, &imp.module)?,
                        );
                    }
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
