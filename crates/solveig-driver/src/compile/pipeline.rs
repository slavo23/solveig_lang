//! Frontend orchestration from project loading through checked modules.
//! The pipeline accumulates diagnostics and retains per-module checked artifacts.

use super::*;

pub(super) struct CheckProjectOutcome {
    pub(super) snapshot: Option<CompilationSnapshot>,
    pub(super) phase: Option<FrontendPhase>,
    pub(super) freshly_checked_modules: Vec<String>,
    pub(super) reused_modules: Vec<String>,
}

pub(super) fn verify_lowering_core(
    merged: &CheckedModule,
    checked_project: &CheckedProject,
) -> Vec<Diagnostic> {
    let lint_env = CoreLintEnv::from_module(merged, &checked_project.names)
        .with_files(checked_project.module_files.clone());
    lint_checked_module(merged, &lint_env)
}

impl Compilation {
    pub(super) fn check_project_modules(
        &mut self,
        verify_core: bool,
    ) -> Result<CheckProjectOutcome, DriverError> {
        let entry_file = find_entry_file(
            &self.project_root,
            self.entry_file.as_deref(),
            &self.source_provider,
        )?;
        let loaded_project =
            load_project_with_source_provider(&self.session, &entry_file, &self.source_provider);
        let (source_root, project) = match loaded_project {
            Ok(project) => project,
            Err(ProjectError::Driver(error)) => return Err(error),
            Err(ProjectError::Resolve(error)) => {
                push_resolve_errors(&self.session, None, vec![error]);
                return Ok(CheckProjectOutcome {
                    snapshot: None,
                    phase: Some(FrontendPhase::Resolve),
                    freshly_checked_modules: Vec::new(),
                    reused_modules: Vec::new(),
                });
            }
        };
        let parsed = parse_project(&project);

        if self.session.diagnostics().has_errors() {
            let source_files = self.session.source_map().files().to_vec();
            return Ok(CheckProjectOutcome {
                snapshot: Some(empty_compilation_snapshot(
                    source_root,
                    project,
                    parsed,
                    self.names.clone(),
                    source_files,
                )),
                phase: Some(FrontendPhase::Parse),
                freshly_checked_modules: Vec::new(),
                reused_modules: Vec::new(),
            });
        }

        let order = match module_order_checked(&self.session, &project) {
            Ok(order) => order,
            Err(error) => {
                push_resolve_errors(&self.session, Some(&project.module_table), vec![error]);
                let source_files = self.session.source_map().files().to_vec();
                return Ok(CheckProjectOutcome {
                    snapshot: Some(empty_compilation_snapshot(
                        source_root,
                        project,
                        parsed,
                        self.names.clone(),
                        source_files,
                    )),
                    phase: Some(FrontendPhase::Resolve),
                    freshly_checked_modules: Vec::new(),
                    reused_modules: Vec::new(),
                });
            }
        };
        let reusable_modules = self
            .package_cache
            .as_ref()
            .map(|cache| cache.reusable_modules(&project, verify_core))
            .unwrap_or_default();

        let caches = (self.package_cache.as_ref(), &reusable_modules);
        let mut state = match FrontendState::new(
            &self.session,
            &mut self.names,
            (&project, &parsed, &order),
            verify_core,
            caches,
        ) {
            Ok(state) => state,
            Err(errors) => {
                push_resolve_errors(&self.session, Some(&project.module_table), errors);
                let source_files = self.session.source_map().files().to_vec();
                return Ok(CheckProjectOutcome {
                    snapshot: Some(empty_compilation_snapshot(
                        source_root,
                        project,
                        parsed,
                        self.names.clone(),
                        source_files,
                    )),
                    phase: Some(FrontendPhase::Resolve),
                    freshly_checked_modules: Vec::new(),
                    reused_modules: Vec::new(),
                });
            }
        };
        let resolve_phase = state.resolve_all(&order);
        let coherence_phase = resolve_phase
            .is_none()
            .then(|| state.validate_instance_graph(&order))
            .flatten();
        let check_phase = (resolve_phase.is_none() && coherence_phase.is_none())
            .then(|| state.check_all(&order))
            .flatten();
        let phase = [resolve_phase, coherence_phase, check_phase]
            .into_iter()
            .flatten()
            .min();

        let source_files = self.session.source_map().files().to_vec();
        let (snapshot, package_cache, freshly_checked_modules, reused_modules) =
            state.into_snapshot(source_root, source_files, &order);
        if let Some(package_cache) = package_cache {
            self.package_cache = Some(package_cache);
        }

        Ok(CheckProjectOutcome {
            snapshot: Some(snapshot),
            phase,
            freshly_checked_modules,
            reused_modules,
        })
    }
}

fn push_resolve_errors(
    session: &Session,
    modules: Option<&ModuleTable>,
    errors: Vec<ResolveError>,
) {
    for error in errors {
        session
            .diagnostics()
            .push(error.into_diagnostic((session, modules)));
    }
}

include!("pipeline/frontend.rs");
include!("pipeline/frontend/cache_artifacts.rs");

fn empty_compilation_snapshot(
    source_root: PathBuf,
    project: Project,
    parsed: ParsedProject,
    names: Names,
    source_files: Vec<SourceFile>,
) -> CompilationSnapshot {
    CompilationSnapshot {
        source_root,
        project,
        parsed,
        names,
        apis: FxHashMap::default(),
        typed_apis: FxHashMap::default(),
        modules: Vec::new(),
        checked: FxHashMap::default(),
        module_files: FxHashMap::default(),
        source_files,
        resolutions: FxHashMap::default(),
        resolve_errors: Vec::new(),
        type_errors: Vec::new(),
        holes: Vec::new(),
    }
}

fn find_entry_file(
    project_root: &Path,
    entry_file: Option<&Path>,
    sources: &SourceProvider,
) -> Result<PathBuf, DriverError> {
    let entry = if let Some(entry_file) = entry_file {
        entry_file.to_path_buf()
    } else if project_root.join(solveig_package::MANIFEST_FILE).is_file() {
        let project_root = camino::Utf8Path::from_path(project_root).ok_or_else(|| {
            solveig_package::PackageError::NonUtf8Path {
                path: project_root.to_path_buf(),
            }
        })?;
        Manifest::read(project_root)?
            .entry_file(project_root)
            .into_std_path_buf()
    } else {
        project_root.join("src").join("main.sv")
    };
    if sources.has_source(&entry) {
        Ok(entry)
    } else {
        Err(DriverError::EntryFileMissing { path: entry })
    }
}

include!("pipeline/merge.rs");
