//! Frontend orchestration from project loading through checked modules.
//! The pipeline accumulates diagnostics and retains per-module checked artifacts.

use super::*;

pub(super) struct CheckProjectOutcome {
    pub(super) checked_project: Option<CheckedProject>,
    pub(super) semantic_tokens: Vec<SourceSemanticToken>,
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
        let cached_prelude_modules = self
            .prelude_cache
            .as_ref()
            .map(|cache| &cache.project_modules);
        let loaded_project = if let Some(cached_prelude_modules) = cached_prelude_modules {
            load_project_with_source_provider(
                &self.session,
                &entry_file,
                &self.source_provider,
                Some(cached_prelude_modules),
            )
        } else {
            load_project_with_source_provider(
                &self.session,
                &entry_file,
                &self.source_provider,
                None,
            )
        };
        let (_root, project) = match loaded_project {
            Ok(project) => project,
            Err(ProjectError::Driver(error)) => return Err(error),
            Err(ProjectError::Resolve(error)) => {
                push_resolve_errors(&self.session, None, vec![error]);
                return Ok(CheckProjectOutcome {
                    checked_project: None,
                    semantic_tokens: Vec::new(),
                    phase: Some(FrontendPhase::Resolve),
                    freshly_checked_modules: Vec::new(),
                    reused_modules: Vec::new(),
                });
            }
        };
        let semantic_tokens = syntax_semantic_tokens(&project);

        if self.session.diagnostics().has_errors() {
            return Ok(CheckProjectOutcome {
                checked_project: None,
                semantic_tokens,
                phase: Some(FrontendPhase::Parse),
                freshly_checked_modules: Vec::new(),
                reused_modules: Vec::new(),
            });
        }

        let order = match module_order_checked(&self.session, &project) {
            Ok(order) => order,
            Err(error) => {
                push_resolve_errors(&self.session, Some(&project.module_table), vec![error]);
                return Ok(CheckProjectOutcome {
                    checked_project: None,
                    semantic_tokens,
                    phase: Some(FrontendPhase::Resolve),
                    freshly_checked_modules: Vec::new(),
                    reused_modules: Vec::new(),
                });
            }
        };
        let parsed = parse_project(&project);
        let prelude_cache = self
            .prelude_cache
            .as_ref()
            .filter(|cache| cache.can_reuse(&project.module_table, verify_core));
        let reusable_modules = self
            .incremental_cache
            .as_ref()
            .map(|cache| cache.reusable_modules(&self.session, &project, verify_core))
            .unwrap_or_default();

        let caches = (
            prelude_cache,
            self.incremental_cache.as_ref(),
            &reusable_modules,
        );
        let mut state = match FrontendState::new(
            &self.session,
            &mut self.names,
            &project,
            &parsed,
            &order,
            verify_core,
            caches,
        ) {
            Ok(state) => state,
            Err(errors) => {
                push_resolve_errors(&self.session, Some(&project.module_table), errors);
                return Ok(CheckProjectOutcome {
                    checked_project: None,
                    semantic_tokens,
                    phase: Some(FrontendPhase::Resolve),
                    freshly_checked_modules: Vec::new(),
                    reused_modules: Vec::new(),
                });
            }
        };
        let phase = state.check_all(&order);

        let source_files = self.session.source_map().files().to_vec();
        let (
            checked_project,
            prelude_cache,
            incremental_cache,
            freshly_checked_modules,
            reused_modules,
        ) = state.into_checked_project(source_files, &order);
        if let Some(prelude_cache) = prelude_cache {
            self.prelude_cache = Some(prelude_cache);
        }
        if let Some(incremental_cache) = incremental_cache {
            self.incremental_cache = Some(incremental_cache);
        }

        Ok(CheckProjectOutcome {
            checked_project: Some(checked_project),
            semantic_tokens,
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
include!("pipeline/frontend/symbol_artifacts.rs");
include!("pipeline/frontend/hover_artifacts.rs");

fn find_entry_file(
    project_root: &Path,
    entry_file: Option<&Path>,
    sources: &SourceProvider,
) -> Result<PathBuf, DriverError> {
    let entry = entry_file.map_or_else(
        || project_root.join("src").join("main.sv"),
        Path::to_path_buf,
    );
    if sources.has_source(&entry) {
        Ok(entry)
    } else {
        Err(DriverError::EntryFileMissing { path: entry })
    }
}

include!("pipeline/merge.rs");
