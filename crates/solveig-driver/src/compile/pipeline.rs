//! Frontend orchestration from project loading through checked modules.
//! The pipeline accumulates diagnostics and retains per-module checked artifacts.

use super::*;

pub(super) struct CheckProjectOutcome {
    pub(super) checked_project: Option<CheckedProject>,
    pub(super) phase: Option<FrontendPhase>,
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
        let entry_file = find_entry_file(&self.project_root, &self.source_provider)?;
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
                    phase: Some(FrontendPhase::Resolve),
                });
            }
        };

        if self.session.diagnostics().has_errors() {
            return Ok(CheckProjectOutcome {
                checked_project: None,
                phase: Some(FrontendPhase::Parse),
            });
        }

        let order = match module_order_checked(&self.session, &project) {
            Ok(order) => order,
            Err(error) => {
                push_resolve_errors(&self.session, Some(&project.module_table), vec![error]);
                return Ok(CheckProjectOutcome {
                    checked_project: None,
                    phase: Some(FrontendPhase::Resolve),
                });
            }
        };
        let parsed = parse_project(&project);
        let prelude_cache = self
            .prelude_cache
            .as_ref()
            .filter(|cache| cache.can_reuse(&project.module_table, verify_core));

        let mut state = match FrontendState::new(
            &self.session,
            &mut self.names,
            &project,
            &parsed,
            &order,
            verify_core,
            prelude_cache,
        ) {
            Ok(state) => state,
            Err(errors) => {
                push_resolve_errors(&self.session, Some(&project.module_table), errors);
                return Ok(CheckProjectOutcome {
                    checked_project: None,
                    phase: Some(FrontendPhase::Resolve),
                });
            }
        };
        let phase = state.check_all(&order);

        let source_files = self.session.source_map().files().to_vec();
        let (checked_project, prelude_cache) = state.into_checked_project(source_files);
        if let Some(prelude_cache) = prelude_cache {
            self.prelude_cache = Some(prelude_cache);
        }

        Ok(CheckProjectOutcome {
            checked_project: Some(checked_project),
            phase,
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

fn find_entry_file(project_root: &Path, sources: &SourceProvider) -> Result<PathBuf, DriverError> {
    let entry = project_root.join("src").join("main.sv");
    if sources.has_source(&entry) {
        Ok(entry)
    } else {
        Err(DriverError::EntryFileMissing { path: entry })
    }
}

include!("pipeline/merge.rs");
