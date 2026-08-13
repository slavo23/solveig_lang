//! Workspace state, published read snapshots, and synchronous project checks.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use async_lsp::lsp_types::{
    CodeActionResponse, CompletionResponse, Diagnostic, DocumentSymbol, Hover, InlayHint, Location,
    Position, Range, SemanticTokens, SymbolInformation, Url,
};
use solveig_base::{PositionEncoding, Session, SourceMap, SourcePosition};
use solveig_driver::{
    Compilation, CompilationSnapshot, DriverError, SourceProvider, compiler_library_location,
};
use solveig_ide::{ProjectSnapshotView, SourceCodeActionCategory, SourceInlayHintConfig};
use solveig_package::{find_nearest_manifest_path, is_package_root};
use tokio::task::JoinHandle;

use crate::code_actions::lsp_code_actions;
use crate::completion::lsp_completion_items;
use crate::diagnostics::{
    diagnostic_map, inferred_signature_diagnostic_map, missing_entry_diagnostic,
};
use crate::hover::lsp_hover;
use crate::inlay_hints::lsp_inlay_hints;
use crate::navigation::{lsp_document_symbols, lsp_location, lsp_symbol_information};
use crate::semantic_tokens::lsp_semantic_tokens;

mod editor_features;

pub(crate) struct ServerState {
    workspaces: BTreeMap<PathBuf, Arc<Mutex<Workspace>>>,
    roots: Vec<PathBuf>,
    previous_diagnostics: BTreeMap<PathBuf, BTreeSet<Url>>,
    published_snapshots: BTreeMap<PathBuf, WorkspaceSnapshot>,
    pub(crate) pending_check: Option<JoinHandle<()>>,
    encoding: PositionEncoding,
    config: ServerConfig,
}

pub(crate) struct Workspace {
    root: PathBuf,
    first_file: PathBuf,
    compilation: Compilation,
    open_files: BTreeMap<PathBuf, String>,
    poisoned: bool,
}

#[derive(Clone)]
pub(crate) struct WorkspaceSnapshot {
    session: Session,
    source_map: SourceMap,
    source_provider: SourceProvider,
    snapshot: Arc<CompilationSnapshot>,
    success: bool,
}

#[derive(Clone)]
pub(crate) struct WorkspaceCheckPlan {
    root: PathBuf,
    workspace: Arc<Mutex<Workspace>>,
}

pub(crate) struct WorkspaceCheckOutcome {
    root: PathBuf,
    diagnostics: BTreeMap<Url, Vec<Diagnostic>>,
    snapshot: Option<WorkspaceSnapshot>,
    failed: bool,
}

enum LspFailure {
    Panic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ServerConfig {
    pub(crate) inlay_hints_enabled: bool,
    pub(crate) inferred_signature_hints_enabled: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            inlay_hints_enabled: true,
            inferred_signature_hints_enabled: false,
        }
    }
}

impl ServerState {
    pub(crate) fn new(encoding: PositionEncoding) -> Self {
        Self::new_with_config(encoding, ServerConfig::default())
    }

    pub(crate) fn new_with_config(encoding: PositionEncoding, config: ServerConfig) -> Self {
        match compiler_library_location() {
            Ok(location) => tracing::info!(
                root = %location.root(),
                origin = location.origin().as_str(),
                "Resolved Solveig compiler library"
            ),
            Err(error) => tracing::error!(%error, "Failed to resolve Solveig compiler library"),
        }
        Self {
            workspaces: BTreeMap::new(),
            roots: Vec::new(),
            previous_diagnostics: BTreeMap::new(),
            published_snapshots: BTreeMap::new(),
            pending_check: None,
            encoding,
            config,
        }
    }

    pub(crate) fn set_roots(&mut self, roots: Vec<PathBuf>) {
        self.roots = roots;
    }

    pub(crate) fn open_document(&mut self, uri: &Url, text: String) -> Option<()> {
        let path = uri.to_file_path().ok()?;
        let workspace = self.workspace_for_path(&path);
        lock_workspace(&workspace).open_document(path, text);
        Some(())
    }

    pub(crate) fn change_document(&mut self, uri: &Url, text: String) -> Option<()> {
        let path = uri.to_file_path().ok()?;
        let workspace = self.workspace_for_path(&path);
        lock_workspace(&workspace).change_document(path, text);
        Some(())
    }

    pub(crate) fn save_document(&mut self, uri: &Url, text: Option<String>) -> Option<()> {
        let path = uri.to_file_path().ok()?;
        let workspace = self.workspace_for_path(&path);
        lock_workspace(&workspace).save_document(path, text);
        Some(())
    }

    pub(crate) fn close_document(&mut self, uri: &Url) -> Option<()> {
        let path = uri.to_file_path().ok()?;
        let workspace = self.workspace_for_path(&path);
        lock_workspace(&workspace).close_document(path);
        Some(())
    }

    pub(crate) fn touch_path(&mut self, path: &Path) {
        self.workspace_for_path(path);
    }

    #[cfg(test)]
    pub(crate) fn check_all(&mut self) -> Vec<(Url, Vec<Diagnostic>)> {
        let outcomes = check_workspaces(self.check_plan(), self.encoding, self.config);
        self.finish_check(outcomes)
    }

    pub(crate) fn check_context(
        &self,
    ) -> (PositionEncoding, ServerConfig, Vec<WorkspaceCheckPlan>) {
        (self.encoding, self.config, self.check_plan())
    }

    pub(crate) fn check_plan(&self) -> Vec<WorkspaceCheckPlan> {
        self.workspaces
            .iter()
            .map(|(root, workspace)| WorkspaceCheckPlan {
                root: root.clone(),
                workspace: Arc::clone(workspace),
            })
            .collect()
    }

    pub(crate) fn finish_check(
        &mut self,
        outcomes: Vec<WorkspaceCheckOutcome>,
    ) -> Vec<(Url, Vec<Diagnostic>)> {
        let mut publications = Vec::new();

        for mut outcome in outcomes {
            if let Some(snapshot) = outcome.snapshot.take() {
                self.published_snapshots
                    .insert(outcome.root.clone(), snapshot);
            }
            publications.extend(self.finish_workspace_check(outcome));
        }

        publications
    }

    pub(crate) fn snapshot_for_uri(
        &self,
        uri: &Url,
    ) -> Option<(PathBuf, PositionEncoding, ServerConfig, WorkspaceSnapshot)> {
        let path = uri.to_file_path().ok()?;
        let root = self.workspace_root_for_path(&path);
        let snapshot = self.published_snapshots.get(&root)?.clone();

        Some((path, self.encoding, self.config, snapshot))
    }

    pub(crate) fn workspace_snapshots(&self) -> (PositionEncoding, Vec<WorkspaceSnapshot>) {
        (
            self.encoding,
            self.published_snapshots.values().cloned().collect(),
        )
    }

    #[cfg(test)]
    pub(crate) fn previous_diagnostic_count(&self) -> usize {
        self.previous_diagnostics.values().map(BTreeSet::len).sum()
    }

    fn workspace_for_path(&mut self, path: &Path) -> Arc<Mutex<Workspace>> {
        let root = self.workspace_root_for_path(path);

        self.workspaces
            .entry(root)
            .or_insert_with_key(|root| Arc::new(Mutex::new(Workspace::new(root, path))))
            .clone()
    }

    fn workspace_root_for_path(&self, path: &Path) -> PathBuf {
        let configured_root = self
            .roots
            .iter()
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count());

        match configured_root {
            Some(root) if is_solveig_project_root(root) => root.clone(),
            Some(_) | None => project_root_for_file(path),
        }
    }

    fn finish_workspace_check(
        &mut self,
        outcome: WorkspaceCheckOutcome,
    ) -> Vec<(Url, Vec<Diagnostic>)> {
        let current_uris = outcome.diagnostics.keys().cloned().collect::<BTreeSet<_>>();
        let mut publications = outcome.diagnostics.into_iter().collect::<Vec<_>>();

        if outcome.failed {
            return publications;
        }

        let previous = self
            .previous_diagnostics
            .remove(&outcome.root)
            .unwrap_or_default();
        for uri in previous.difference(&current_uris) {
            publications.push((uri.clone(), Vec::new()));
        }
        self.previous_diagnostics.insert(outcome.root, current_uris);

        publications
    }
}

#[cfg(test)]
impl ServerState {
    pub(crate) fn hover(&self, uri: &Url, position: Position) -> Option<Hover> {
        let (path, encoding, _config, snapshot) = self.snapshot_for_uri(uri)?;

        snapshot.hover(&path, position, encoding)
    }

    pub(crate) fn code_actions(&self, uri: &Url, range: Range) -> Option<CodeActionResponse> {
        let (path, encoding, config, snapshot) = self.snapshot_for_uri(uri)?;

        snapshot.code_actions(
            &path,
            range,
            encoding,
            config.inferred_signature_hints_enabled,
        )
    }
}

impl Workspace {
    fn new(root: &Path, first_file: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            first_file: first_file.to_path_buf(),
            compilation: compilation_for(root, first_file),
            open_files: BTreeMap::new(),
            poisoned: false,
        }
    }

    fn open_document(&mut self, path: PathBuf, text: String) {
        self.open_files.insert(path.clone(), text.clone());
        self.compilation.set_overlay(path, text);
    }

    fn change_document(&mut self, path: PathBuf, text: String) {
        self.open_files.insert(path.clone(), text.clone());
        self.compilation.set_overlay(path, text);
    }

    fn save_document(&mut self, path: PathBuf, text: Option<String>) {
        if let Some(text) = text {
            self.open_files.insert(path.clone(), text.clone());
            self.compilation.set_overlay(path, text);
        }
    }

    fn close_document(&mut self, path: PathBuf) {
        self.open_files.remove(&path);
        self.compilation.clear_overlay(path);
    }

    fn check(&mut self, encoding: PositionEncoding, config: ServerConfig) -> WorkspaceCheckOutcome {
        if self.poisoned {
            self.rebuild_compilation();
        }

        let root = self.root.clone();
        let started = Instant::now();
        match guarded("workspace check", || self.check_inner(encoding, config)) {
            Ok(Ok(outcome)) => {
                self.poisoned = false;
                tracing::info!(
                    root = %root.display(),
                    elapsed_ms = started.elapsed().as_millis(),
                    failed = false,
                    "Solveig workspace check completed"
                );
                outcome
            }
            Ok(Err(DriverError::EntryFileMissing { path })) => {
                tracing::warn!(
                    root = %root.display(),
                    path = %path.display(),
                    "Solveig workspace entry file is missing"
                );
                WorkspaceCheckOutcome {
                    root,
                    diagnostics: missing_entry_diagnostic(&path).into_iter().collect(),
                    snapshot: None,
                    failed: false,
                }
            }
            Ok(Err(err)) => {
                tracing::warn!(
                    root = %root.display(),
                    error = %err,
                    "Solveig workspace check failed before diagnostics were available"
                );
                WorkspaceCheckOutcome {
                    root,
                    diagnostics: BTreeMap::new(),
                    snapshot: None,
                    failed: true,
                }
            }
            Err(LspFailure::Panic) => {
                self.poisoned = true;
                WorkspaceCheckOutcome {
                    root,
                    diagnostics: BTreeMap::new(),
                    snapshot: None,
                    failed: true,
                }
            }
        }
    }

    fn check_inner(
        &mut self,
        encoding: PositionEncoding,
        config: ServerConfig,
    ) -> Result<WorkspaceCheckOutcome, DriverError> {
        let (mut diagnostics, source_map, success, fresh, reused) = {
            let result = self.compilation.check(false)?;
            (
                diagnostic_map(result.diagnostics(), result.source_map(), encoding),
                result.source_map().clone(),
                result.success(),
                result.freshly_checked_modules().len(),
                result.reused_modules().len(),
            )
        };
        let snapshot = self.compilation.snapshot().map(|snapshot| {
            WorkspaceSnapshot::from_compilation(
                &self.compilation,
                source_map.clone(),
                snapshot,
                success,
            )
        });

        if config.inferred_signature_hints_enabled
            && let Some(snapshot) = snapshot.as_ref()
        {
            extend_diagnostic_map(
                &mut diagnostics,
                inferred_signature_diagnostic_map(
                    &snapshot.source_code_actions(),
                    &snapshot.source_map,
                    encoding,
                ),
            );
        }

        tracing::info!(
            root = %self.root.display(),
            fresh,
            reused,
            success,
            "Solveig workspace check retained snapshot"
        );

        Ok(WorkspaceCheckOutcome {
            root: self.root.clone(),
            diagnostics,
            snapshot,
            failed: false,
        })
    }

    fn rebuild_compilation(&mut self) {
        tracing::info!(
            root = %self.root.display(),
            "Rebuilding poisoned Solveig workspace compilation"
        );
        self.compilation = compilation_for(&self.root, &self.first_file);
        for (path, text) in &self.open_files {
            self.compilation.set_overlay(path, text.clone());
        }
        self.poisoned = false;
    }
}

impl WorkspaceSnapshot {
    fn from_compilation(
        compilation: &Compilation,
        source_map: SourceMap,
        snapshot: &CompilationSnapshot,
        success: bool,
    ) -> Self {
        Self {
            session: compilation.session().clone(),
            source_map,
            source_provider: compilation.source_provider().clone(),
            snapshot: Arc::new(snapshot.clone()),
            success,
        }
    }

    fn view(&self) -> ProjectSnapshotView<'_> {
        ProjectSnapshotView::new(
            &self.session,
            &self.source_map,
            &self.source_provider,
            &self.snapshot,
            self.success,
        )
    }

    fn source_code_actions(&self) -> Vec<solveig_ide::SourceCodeAction> {
        self.view().source_code_actions()
    }

    pub(crate) fn hover(
        &self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
    ) -> Option<Hover> {
        guarded("hover request", || {
            let info = self.view().hover_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
            )?;

            Some(lsp_hover(&info, &self.source_map, encoding))
        })
        .ok()
        .flatten()
    }

    pub(crate) fn definition(
        &self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
    ) -> Option<Location> {
        guarded("definition request", || {
            let span = self.view().definition_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
            )?;

            lsp_location(span, &self.source_map, encoding)
        })
        .ok()
        .flatten()
    }

    pub(crate) fn references(
        &self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
        guarded("references request", || {
            let spans = self.view().references_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
                include_declaration,
            )?;

            Some(
                spans
                    .into_iter()
                    .filter_map(|span| lsp_location(span, &self.source_map, encoding))
                    .collect(),
            )
        })
        .ok()
        .flatten()
    }

    pub(crate) fn completion(
        &self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
    ) -> Option<CompletionResponse> {
        guarded("completion request", || {
            let items = self.view().completion_items_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
            );

            Some(CompletionResponse::Array(lsp_completion_items(&items)))
        })
        .ok()
        .flatten()
    }

    pub(crate) fn inlay_hints(
        &self,
        path: &Path,
        range: Range,
        encoding: PositionEncoding,
    ) -> Option<Vec<InlayHint>> {
        guarded("inlay-hints request", || {
            let hints = self.view().inlay_hints_at_range(
                path,
                SourcePosition::new(range.start.line, range.start.character),
                SourcePosition::new(range.end.line, range.end.character),
                encoding,
                SourceInlayHintConfig::default(),
            );

            Some(lsp_inlay_hints(&hints, &self.source_map, encoding))
        })
        .ok()
        .flatten()
    }

    pub(crate) fn document_symbols(
        &self,
        path: &Path,
        encoding: PositionEncoding,
    ) -> Option<Vec<DocumentSymbol>> {
        guarded("document-symbol request", || {
            let symbols = self.view().document_symbols(path);
            Some(lsp_document_symbols(&symbols, &self.source_map, encoding))
        })
        .ok()
        .flatten()
    }

    pub(crate) fn workspace_symbols(
        &self,
        query: &str,
        encoding: PositionEncoding,
    ) -> Vec<SymbolInformation> {
        guarded("workspace-symbol request", || {
            let symbols = self.view().workspace_symbols(query);
            lsp_symbol_information(&symbols, &self.source_map, encoding)
        })
        .unwrap_or_default()
    }

    pub(crate) fn semantic_tokens_full(
        &self,
        path: &Path,
        encoding: PositionEncoding,
    ) -> Option<SemanticTokens> {
        guarded("semantic-tokens request", || {
            let tokens = self.view().semantic_tokens(path);
            Some(SemanticTokens {
                result_id: None,
                data: lsp_semantic_tokens(&tokens, &self.source_map, encoding),
            })
        })
        .ok()
        .flatten()
    }

    pub(crate) fn code_actions(
        &self,
        path: &Path,
        range: Range,
        encoding: PositionEncoding,
        include_inferred_signatures: bool,
    ) -> Option<CodeActionResponse> {
        guarded("code-action request", || {
            let mut actions = self.view().code_actions_at_range(
                path,
                SourcePosition::new(range.start.line, range.start.character),
                SourcePosition::new(range.end.line, range.end.character),
                encoding,
            );
            if !include_inferred_signatures {
                actions.retain(|action| {
                    action.category() != SourceCodeActionCategory::InferredSignature
                });
            }

            Some(lsp_code_actions(&actions, &self.source_map, encoding))
        })
        .ok()
        .flatten()
    }
}

pub(crate) fn check_workspaces(
    plan: Vec<WorkspaceCheckPlan>,
    encoding: PositionEncoding,
    config: ServerConfig,
) -> Vec<WorkspaceCheckOutcome> {
    plan.into_iter()
        .map(|item| {
            tracing::debug!(root = %item.root.display(), "Checking Solveig workspace");
            lock_workspace(&item.workspace).check(encoding, config)
        })
        .collect()
}

fn lock_workspace(workspace: &Mutex<Workspace>) -> MutexGuard<'_, Workspace> {
    workspace
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn guarded<T>(operation: &'static str, f: impl FnOnce() -> T) -> Result<T, LspFailure> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => Ok(value),
        Err(payload) => {
            tracing::error!(
                operation,
                panic = %panic_message(payload.as_ref()),
                "Solveig LSP operation panicked"
            );
            Err(LspFailure::Panic)
        }
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

fn extend_diagnostic_map(
    left: &mut BTreeMap<Url, Vec<Diagnostic>>,
    right: BTreeMap<Url, Vec<Diagnostic>>,
) {
    for (uri, mut diagnostics) in right {
        left.entry(uri).or_default().append(&mut diagnostics);
    }
}

fn project_root_for_file(path: &Path) -> PathBuf {
    let source_dir = path.parent().unwrap_or_else(|| Path::new("."));

    find_nearest_manifest_path(path).unwrap_or_else(|| source_dir.to_path_buf())
}

fn is_solveig_project_root(path: &Path) -> bool {
    is_package_root(path)
}

fn compilation_for(root: &Path, first_file: &Path) -> Compilation {
    if is_package_root(root) {
        Compilation::new(root)
    } else {
        Compilation::for_source_file(first_file)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::diagnostics::INFERRED_SIGNATURE_DIAGNOSTIC_CODE;
    use async_lsp::lsp_types::{
        CodeActionOrCommand, DiagnosticSeverity, HoverContents, NumberOrString,
    };
    use solveig_driver::Compilation;
    use std::fs;
    use std::ops::Deref;
    use std::process::Command;
    use tempfile::{Builder, TempDir};

    const BROKEN_PRELUDE_CHILD: &str = "SOLVEIG_S2_BROKEN_PRELUDE_CHILD";
    const BROKEN_PRELUDE_PROJECT: &str = "SOLVEIG_S2_BROKEN_PRELUDE_PROJECT";

    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("test crate should live under workspace/crates")
            .to_path_buf()
    }

    fn golden_cases() -> Vec<PathBuf> {
        let root = workspace_root().join("tests/golden/diag");
        let mut cases = fs::read_dir(&root)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", root.display()))
            .map(|entry| {
                entry
                    .unwrap_or_else(|err| panic!("failed to read diagnostic case: {err}"))
                    .path()
            })
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        cases.sort();
        cases
    }

    struct TestRoot {
        dir: TempDir,
    }

    impl TestRoot {
        fn path(&self) -> &Path {
            self.dir.path()
        }
    }

    impl Deref for TestRoot {
        type Target = Path;

        fn deref(&self) -> &Self::Target {
            self.path()
        }
    }

    impl AsRef<Path> for TestRoot {
        fn as_ref(&self) -> &Path {
            self.path()
        }
    }

    fn temp_root(name: &str) -> TestRoot {
        let dir = Builder::new()
            .prefix(&format!("solveig_lsp_{name}_"))
            .tempdir()
            .expect("test temp directory should be creatable");
        TestRoot { dir }
    }

    fn write_source(path: &Path, source: &str) {
        fs::create_dir_all(
            path.parent()
                .expect("test source should have a parent directory"),
        )
        .expect("test source directory should be creatable");
        fs::write(path, source).expect("test source should be writable");
    }

    fn write_broken_library(root: &Path) {
        let shipped = workspace_root().join("lib");
        for relative in [
            "prelude/solveig.toml",
            "prelude/src/prelude/builtins.sv",
            "prelude/src/prelude/list.sv",
            "prelude/src/prelude/prim.sv",
            "prelude/src/prelude/typeclasses.sv",
            "std/solveig.toml",
            "std/src/std.sv",
        ] {
            let destination = root.join(relative);
            fs::create_dir_all(
                destination
                    .parent()
                    .expect("library fixture path should have a parent"),
            )
            .expect("library fixture directory should be creatable");
            fs::copy(shipped.join(relative), &destination)
                .expect("shipped library file should copy into the fixture");
        }

        let list = root.join("prelude/src/prelude/list.sv");
        let source = fs::read_to_string(&list).expect("fixture list source should be readable");
        let broken = source.replacen(
            "map : forall a b. (a -> b) -> [a] -> [b]",
            "map : MissingType",
            1,
        );
        assert_ne!(broken, source, "Prelude fixture mutation should apply");
        fs::write(list, broken).expect("broken list fixture should be writable");
    }

    #[test]
    fn golden_diagnostic_cases_convert_codes_and_ranges() {
        for case_root in golden_cases() {
            let case_name = case_root
                .file_name()
                .and_then(|name| name.to_str())
                .expect("diagnostic case should have a UTF-8 name");
            let mut state = ServerState::new(PositionEncoding::Utf16);
            state.set_roots(vec![case_root.clone()]);
            state.touch_path(&case_root.join("src/main.sv"));

            let publications = state.check_all();

            if case_name == "missing_entry" {
                assert_eq!(publications.len(), 1, "missing_entry should publish once");
                assert_eq!(
                    publications[0].1[0].code,
                    Some(NumberOrString::String("DRV0001".to_string()))
                );
                assert_eq!(publications[0].1[0].range.start.line, 0);
                assert_eq!(publications[0].1[0].range.start.character, 0);
                continue;
            }

            let mut compilation = Compilation::new(&case_root);
            let result = compilation
                .check(false)
                .unwrap_or_else(|err| panic!("{case_name} should return diagnostics: {err}"));
            let expected = diagnostic_map(
                result.diagnostics(),
                result.source_map(),
                PositionEncoding::Utf16,
            );
            let actual = publications.into_iter().collect::<BTreeMap<_, _>>();

            assert_eq!(
                diagnostic_keys(&actual),
                diagnostic_keys(&expected),
                "{case_name} LSP diagnostics changed"
            );
        }
    }

    #[test]
    fn text_document_lifecycle_clears_previous_diagnostics() {
        let project_root = temp_root("clear");
        let source = project_root.join("scratch.sv");
        write_source(
            &source,
            r#"main : Int
main = True
"#,
        );
        let uri = Url::from_file_path(&source).expect("test path should become a file URI");
        let mut state = ServerState::new(PositionEncoding::Utf16);

        state.open_document(
            &uri,
            r#"main : Int
main = True
"#
            .to_string(),
        );
        let first = state.check_all();
        assert_eq!(first.len(), 1);
        assert_eq!(
            first[0].1[0].code,
            Some(NumberOrString::String("TC0005".to_string()))
        );

        state.change_document(
            &uri,
            r#"main : Int
main = 1
"#
            .to_string(),
        );
        let second = state.check_all();
        assert_eq!(second, vec![(uri, Vec::new())]);

        drop(project_root);
    }

    #[test]
    fn repeated_document_edits_keep_retained_lsp_state_bounded() {
        let project_root = temp_root("bounded_edits");
        let source = project_root.join("scratch.sv");
        let initial = "main : Int\nmain = 0\n";
        write_source(&source, initial);
        let uri = Url::from_file_path(&source).expect("test path should become a file URI");
        let mut state = ServerState::new(PositionEncoding::Utf16);

        state.open_document(&uri, initial.to_string());
        let initial_publications = state.check_all();
        assert!(
            diagnostics_for_uri(&initial_publications, &uri).is_empty(),
            "initial edit-memory fixture should check: {initial_publications:#?}"
        );
        let baseline = retained_lsp_counts(&state, &uri);

        for edit_index in 0..1_000 {
            let edited = format!("main : Int\nmain = {}\n", edit_index % 97);
            state.change_document(&uri, edited);
            let publications = state.check_all();
            assert!(
                diagnostics_for_uri(&publications, &uri).is_empty(),
                "edit {edit_index} should check without diagnostics: {publications:#?}"
            );
        }

        let final_counts = retained_lsp_counts(&state, &uri);
        assert_eq!(
            final_counts.workspaces, baseline.workspaces,
            "workspace table should not grow over repeated edits"
        );
        assert_eq!(
            final_counts.open_files, baseline.open_files,
            "open-file overlays should replace the previous text"
        );
        assert_eq!(
            final_counts.published_snapshots, baseline.published_snapshots,
            "published snapshots should be replaced per workspace"
        );
        assert_eq!(
            final_counts.source_map_files, baseline.source_map_files,
            "published source maps should not retain previous edits"
        );
        assert_eq!(
            final_counts.snapshot_source_files, baseline.snapshot_source_files,
            "compilation snapshots should not retain previous edits"
        );

        let hover = state
            .hover(&uri, Position::new(1, 7))
            .expect("final edited snapshot should still answer hovers");
        assert_hover_contains(hover, "Int");

        drop(project_root);
    }

    #[test]
    fn typed_hole_diagnostic_publishes_for_open_file() {
        let project_root = temp_root("hole_diagnostic");
        let source = project_root.join("scratch.sv");
        let text = r#"main : Bool
main = ??
"#;
        write_source(&source, text);
        let uri = Url::from_file_path(&source).expect("test path should become a file URI");
        let mut state = ServerState::new(PositionEncoding::Utf16);

        state.open_document(&uri, text.to_string());
        let publications = state.check_all();
        let diagnostics = diagnostics_for_uri(&publications, &uri);
        let hole = diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.code.as_ref() == Some(&NumberOrString::String("HOLE".to_string()))
            })
            .unwrap_or_else(|| panic!("missing typed-hole diagnostic: {diagnostics:#?}"));

        assert_eq!(hole.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(
            hole.range,
            Range::new(Position::new(1, 7), Position::new(1, 9))
        );
        assert!(
            hole.message.contains("found hole of type `Bool`"),
            "unexpected hole message: {}",
            hole.message
        );

        drop(project_root);
    }

    #[test]
    fn typed_hole_hover_uses_hole_range() {
        let project_root = temp_root("hole_hover");
        let source = project_root.join("scratch.sv");
        let text = r#"main : Bool
main = if True then ?? else False
"#;
        write_source(&source, text);
        let uri = Url::from_file_path(&source).expect("test path should become a file URI");
        let mut state = ServerState::new(PositionEncoding::Utf16);

        state.open_document(&uri, text.to_string());
        let _publications = state.check_all();
        let hover = state
            .hover(&uri, Position::new(1, 20))
            .expect("typed hole should have LSP hover");

        assert_hover_contains(hover.clone(), "**Unfilled hole**");
        assert_hover_contains(hover.clone(), "Expected type: `Bool`");
        assert_eq!(
            hover.range,
            Some(Range::new(Position::new(1, 20), Position::new(1, 22)))
        );

        drop(project_root);
    }

    #[test]
    fn underscore_hole_diagnostic_and_hover_use_hole_range() {
        let project_root = temp_root("underscore_hole");
        let source = project_root.join("scratch.sv");
        let text = r#"main : Bool
main = _
"#;
        write_source(&source, text);
        let uri = Url::from_file_path(&source).expect("test path should become a file URI");
        let mut state = ServerState::new(PositionEncoding::Utf16);

        state.open_document(&uri, text.to_string());
        let publications = state.check_all();
        let diagnostics = diagnostics_for_uri(&publications, &uri);
        let hole = diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.code.as_ref() == Some(&NumberOrString::String("HOLE".to_string()))
            })
            .unwrap_or_else(|| panic!("missing underscore-hole diagnostic: {diagnostics:#?}"));

        assert_eq!(
            hole.range,
            Range::new(Position::new(1, 7), Position::new(1, 8))
        );
        assert_eq!(hole.severity, Some(DiagnosticSeverity::ERROR));

        let hover = state
            .hover(&uri, Position::new(1, 7))
            .expect("underscore hole should have LSP hover");

        assert_hover_contains(hover.clone(), "**Unfilled hole**");
        assert_hover_contains(hover.clone(), "Expected type: `Bool`");
        assert_eq!(
            hover.range,
            Some(Range::new(Position::new(1, 7), Position::new(1, 8)))
        );

        drop(project_root);
    }

    #[test]
    fn nested_constructor_hole_hover_uses_hole_range() {
        let project_root = workspace_root().join("examples/smoke_suite");
        let source = project_root.join("src/main.sv");
        let original = fs::read_to_string(&source).expect("smoke suite main source should load");
        let text = original.replace("Entry iVal iVal", "Entry ?? iVal");
        let uri = Url::from_file_path(&source).expect("test path should become a file URI");
        let mut state = ServerState::new(PositionEncoding::Utf16);
        let line = text
            .lines()
            .position(|line| line.contains("Entry ?? iVal"))
            .expect("smoke suite overlay should contain the nested hole");
        let character = text
            .lines()
            .nth(line)
            .and_then(|line| line.find("??"))
            .expect("nested hole line should contain the hole");
        let position = Position::new(
            u32::try_from(line).expect("test line should fit in u32"),
            u32::try_from(character).expect("test character should fit in u32"),
        );

        state.open_document(&uri, text);
        let publications = state.check_all();
        let diagnostics = diagnostics_for_uri(&publications, &uri);
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code.as_ref() == Some(&NumberOrString::String("HOLE".to_string()))
                    && diagnostic.severity == Some(DiagnosticSeverity::ERROR)
            }),
            "missing nested-hole diagnostic: {diagnostics:#?}"
        );

        let hover = state
            .hover(&uri, position)
            .expect("nested constructor hole should have LSP hover");

        assert_hover_contains(hover.clone(), "**Unfilled hole**");
        assert_hover_contains(hover.clone(), "Expected type: `Int`");
        assert_eq!(
            hover.range,
            Some(Range::new(
                position,
                Position::new(position.line, position.character + 2)
            ))
        );
    }

    #[test]
    fn hole_diagnostic_and_rich_hover_survive_save_recheck() {
        let project_root = workspace_root().join("examples/smoke_suite");
        let source = project_root.join("src/main.sv");
        let original = fs::read_to_string(&source).expect("smoke suite main source should load");
        let text = original.replace("Entry iVal iVal", "Entry iVal ??");
        let uri = Url::from_file_path(&source).expect("test path should become a file URI");
        let mut state = ServerState::new(PositionEncoding::Utf16);
        let line = text
            .lines()
            .position(|line| line.contains("Entry iVal ??"))
            .expect("smoke suite overlay should contain the second-arg hole");
        let character = text
            .lines()
            .nth(line)
            .and_then(|line| line.find("??"))
            .expect("second-arg hole line should contain the hole");
        let position = Position::new(
            u32::try_from(line).expect("test line should fit in u32"),
            u32::try_from(character).expect("test character should fit in u32"),
        );

        state.open_document(&uri, text.clone());
        let first_publications = state.check_all();
        let first_diagnostics = diagnostics_for_uri(&first_publications, &uri);
        let first_hole = first_diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.code.as_ref() == Some(&NumberOrString::String("HOLE".to_string()))
            })
            .unwrap_or_else(|| panic!("missing first hole diagnostic: {first_diagnostics:#?}"));
        assert_eq!(first_hole.severity, Some(DiagnosticSeverity::ERROR));
        assert!(
            first_hole.message.contains("valid fills include: iVal"),
            "first diagnostic should include fill suggestions: {}",
            first_hole.message
        );

        state.save_document(&uri, Some(text));
        let saved_publications = state.check_all();
        let saved_diagnostics = diagnostics_for_uri(&saved_publications, &uri);
        let saved_hole = saved_diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.code.as_ref() == Some(&NumberOrString::String("HOLE".to_string()))
            })
            .unwrap_or_else(|| panic!("missing saved hole diagnostic: {saved_diagnostics:#?}"));
        assert_eq!(saved_hole.severity, Some(DiagnosticSeverity::ERROR));
        assert!(
            saved_hole.message.contains("valid fills include: iVal"),
            "saved diagnostic should include fill suggestions: {}",
            saved_hole.message
        );

        let hover = state
            .hover(&uri, position)
            .expect("saved second-arg constructor hole should have LSP hover");

        assert_hover_contains(hover.clone(), "**Unfilled hole**");
        assert_hover_contains(hover.clone(), "found hole of type `Int`");
        assert_hover_contains(hover.clone(), "Bindings in scope");
        assert_hover_contains(hover.clone(), "iVal : Int");
        assert_hover_contains(hover.clone(), "Valid fills include");
        assert_hover_contains(hover.clone(), "`iVal`");
        assert_eq!(
            hover.range,
            Some(Range::new(
                position,
                Position::new(position.line, position.character + 2)
            ))
        );
    }

    #[test]
    fn inferred_signature_hints_are_configurable_editor_diagnostics() {
        let project_root = temp_root("signature_hints");
        let source = project_root.join("scratch.sv");
        let text = r#"answer = 42

main : Int
main = answer
"#;
        write_source(&source, text);
        let uri = Url::from_file_path(&source).expect("test path should become a file URI");
        let enabled_config = ServerConfig {
            inferred_signature_hints_enabled: true,
            ..ServerConfig::default()
        };
        let disabled_config = ServerConfig {
            inferred_signature_hints_enabled: false,
            ..ServerConfig::default()
        };

        let mut enabled = ServerState::new_with_config(PositionEncoding::Utf16, enabled_config);
        enabled.open_document(&uri, text.to_string());
        let enabled_publications = enabled.check_all();
        let enabled_diagnostics = diagnostics_for_uri(&enabled_publications, &uri);
        assert!(
            enabled_diagnostics.iter().any(|diagnostic| {
                diagnostic.severity == Some(DiagnosticSeverity::INFORMATION)
                    && diagnostic.code.as_ref()
                        == Some(&NumberOrString::String(
                            INFERRED_SIGNATURE_DIAGNOSTIC_CODE.to_string(),
                        ))
                    && diagnostic.range == Range::new(Position::new(0, 0), Position::new(0, 6))
            }),
            "missing inferred-signature hint: {enabled_diagnostics:#?}"
        );
        let enabled_actions = enabled
            .code_actions(&uri, Range::new(Position::new(0, 0), Position::new(0, 6)))
            .expect("code actions should be available");
        assert!(
            enabled_actions.iter().any(|action| matches!(
                action,
                CodeActionOrCommand::CodeAction(action)
                    if action.title == "Add type signature for `answer`"
            )),
            "missing inferred-signature quick fix: {enabled_actions:#?}"
        );

        let mut disabled = ServerState::new_with_config(PositionEncoding::Utf16, disabled_config);
        disabled.open_document(&uri, text.to_string());
        let disabled_publications = disabled.check_all();
        let disabled_diagnostics = diagnostics_for_uri(&disabled_publications, &uri);
        assert!(
            disabled_diagnostics.iter().all(|diagnostic| {
                diagnostic.code.as_ref()
                    != Some(&NumberOrString::String(
                        INFERRED_SIGNATURE_DIAGNOSTIC_CODE.to_string(),
                    ))
            }),
            "disabled inferred-signature hints leaked: {disabled_diagnostics:#?}"
        );
        let disabled_actions = disabled
            .code_actions(&uri, Range::new(Position::new(0, 0), Position::new(0, 6)))
            .expect("code actions should be available");
        assert!(
            disabled_actions.iter().all(|action| !matches!(
                action,
                CodeActionOrCommand::CodeAction(action)
                    if action.title == "Add type signature for `answer`"
            )),
            "disabled inferred-signature quick fix leaked: {disabled_actions:#?}"
        );

        drop(project_root);
    }

    #[test]
    fn inferred_signature_hint_survives_save_and_file_watcher() {
        let project_root = temp_root("signature_hint_save");
        let source = project_root.join("scratch.sv");
        let initial = r#"count : List Int -> Int
count xs =
    case xs of
        [] => 0
        | _ :: rest => 1 + count rest

main : Int
main = count [1, 2, 3]
"#;
        let edited = r#"count xs =
    case xs of
        [] => 0
        | _ :: rest => 1 + count rest

main : Int
main = count [1, 2, 3]
"#;
        write_source(&source, initial);
        let uri = Url::from_file_path(&source).expect("test path should become a file URI");
        let config = ServerConfig {
            inferred_signature_hints_enabled: true,
            ..ServerConfig::default()
        };
        let mut state = ServerState::new_with_config(PositionEncoding::Utf16, config);

        state.open_document(&uri, initial.to_string());
        let initial_publications = state.check_all();
        assert!(
            diagnostics_for_uri(&initial_publications, &uri)
                .iter()
                .all(|diagnostic| diagnostic.code.as_ref()
                    != Some(&NumberOrString::String(
                        INFERRED_SIGNATURE_DIAGNOSTIC_CODE.to_string(),
                    ))),
            "written signature should not produce inferred-signature hint"
        );

        state.change_document(&uri, edited.to_string());
        let changed_publications = state.check_all();
        assert!(
            diagnostics_for_uri(&changed_publications, &uri)
                .iter()
                .any(|diagnostic| diagnostic.code.as_ref()
                    == Some(&NumberOrString::String(
                        INFERRED_SIGNATURE_DIAGNOSTIC_CODE.to_string(),
                    ))),
            "editing out the signature should produce a hint: {changed_publications:#?}"
        );

        write_source(&source, edited);
        state.save_document(&uri, None);
        state.touch_path(&source);
        let saved_publications = state.check_all();
        assert!(
            diagnostics_for_uri(&saved_publications, &uri)
                .iter()
                .any(|diagnostic| diagnostic.code.as_ref()
                    == Some(&NumberOrString::String(
                        INFERRED_SIGNATURE_DIAGNOSTIC_CODE.to_string(),
                    ))),
            "saved signature-less recursive function should keep the hint: {saved_publications:#?}"
        );

        let hover = state
            .hover(&uri, Position::new(0, 0))
            .expect("hover should survive saving a signature-less recursive function");
        assert_hover_contains(hover, "count : forall a. [a] -> Int");

        drop(project_root);
    }

    #[test]
    fn shipped_prelude_file_opens_as_canonical_module() {
        let source = workspace_root().join("lib/prelude/src/prelude/typeclasses.sv");
        let text = fs::read_to_string(&source).expect("shipped typeclasses source should load");
        let uri = Url::from_file_path(&source).expect("test path should become a file URI");
        let target_line = text
            .lines()
            .position(|line| line.contains("compare x y = intCompare x y"))
            .expect("typeclasses source should contain the Int Ord instance");
        let target_character = text
            .lines()
            .nth(target_line)
            .and_then(|line| line.find("intCompare"))
            .expect("target line should contain intCompare");
        let mut state = ServerState::new(PositionEncoding::Utf16);

        state.open_document(&uri, text.clone());
        let initial_publications = state.check_all();
        assert!(
            diagnostics_for_uri(&initial_publications, &uri).is_empty(),
            "direct shipped prelude diagnostics should stay clean before hover: {initial_publications:#?}"
        );
        let hover = state
            .hover(
                &uri,
                Position::new(
                    u32::try_from(target_line).expect("test line should fit in u32"),
                    u32::try_from(target_character).expect("test character should fit in u32"),
                ),
            )
            .expect("hover should work in directly opened shipped prelude");
        assert_hover_contains(hover, "intCompare : Int -> Int -> Ordering");

        let checked_publications = state.check_all();
        assert!(
            diagnostics_for_uri(&checked_publications, &uri).is_empty(),
            "direct shipped prelude diagnostics should stay clean after recheck: {checked_publications:#?}"
        );
        for (line_text, needle, expected) in [
            (
                "compare x y = charCompare x y",
                "charCompare",
                "charCompare : Char -> Char -> Ordering",
            ),
            (
                "compare x y = stringCompare x y",
                "stringCompare",
                "stringCompare : String -> String -> Ordering",
            ),
            (
                "getArgs = prelude.prim.getArgs",
                "prelude.prim.getArgs",
                "getArgs : forall e. IO e [String]",
            ),
            (
                "apply fs xs = concat (map (fun f => map f xs) fs)",
                "map",
                "map : forall a b. (a -> b) -> [a] -> [b]",
            ),
            (
                "mconcat xs = foldr appendS empty xs",
                "foldr",
                "foldr : forall a b. (a -> b -> b) -> b -> [a] -> b",
            ),
        ] {
            let line = text
                .lines()
                .position(|line| line.contains(line_text))
                .unwrap_or_else(|| panic!("typeclasses source should contain `{line_text}`"));
            let character = text
                .lines()
                .nth(line)
                .and_then(|line| line.find(needle))
                .unwrap_or_else(|| panic!("target line should contain `{needle}`"));
            let hover = state
                .hover(
                    &uri,
                    Position::new(
                        u32::try_from(line).expect("test line should fit in u32"),
                        u32::try_from(character).expect("test character should fit in u32"),
                    ),
                )
                .unwrap_or_else(|| {
                    panic!("hover should survive prelude-cache reuse for `{needle}`")
                });
            assert_hover_contains(hover, expected);
        }

        let edited = text.replacen("compare x y = intCompare x y", "compare x y = True", 1);
        state.change_document(&uri, edited);
        let edited_publications = state.check_all();
        let edited_diagnostics = diagnostics_for_uri(&edited_publications, &uri);
        assert!(
            edited_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == Some(DiagnosticSeverity::ERROR)),
            "direct shipped prelude edits should invalidate stale cache: {edited_publications:#?}"
        );
    }

    #[test]
    fn non_project_workspace_root_uses_nested_solveig_project_roots() {
        let container = temp_root("nested_roots");
        let first_root = container.join("first");
        let second_root = container.join("second");
        let first_main = first_root.join("src/main.sv");
        let first_helper = first_root.join("src/helper.sv");
        let second_main = second_root.join("src/main.sv");
        let second_helper = second_root.join("src/helper.sv");

        write_source(
            &first_root.join("solveig.toml"),
            r#"[package]
name = "first"
version = "0.1.0"

[language]
entry = "main"
"#,
        );
        write_source(
            &second_root.join("solveig.toml"),
            r#"[package]
name = "second"
version = "0.1.0"

[language]
entry = "main"
"#,
        );

        write_source(
            &first_main,
            r#"import helper(firstAnswer)

main : Int
main = firstAnswer
"#,
        );
        write_source(
            &first_helper,
            r#"firstAnswer : Int
firstAnswer = 1
"#,
        );
        write_source(
            &second_main,
            r#"import helper(secondAnswer)

main : Int
main = secondAnswer
"#,
        );
        write_source(
            &second_helper,
            r#"secondAnswer : Int
secondAnswer = 2
"#,
        );

        let first_uri = Url::from_file_path(&first_main).expect("first URI should be valid");
        let second_uri = Url::from_file_path(&second_main).expect("second URI should be valid");
        let mut state = ServerState::new(PositionEncoding::Utf16);
        let container_path = container.path().to_path_buf();
        state.set_roots(vec![container_path.clone()]);

        state.open_document(
            &first_uri,
            fs::read_to_string(&first_main).expect("first main should be readable"),
        );
        state.open_document(
            &second_uri,
            fs::read_to_string(&second_main).expect("second main should be readable"),
        );

        assert!(state.workspaces.contains_key(&first_root));
        assert!(state.workspaces.contains_key(&second_root));
        assert!(!state.workspaces.contains_key(&container_path));
        let publications = state.check_all();

        let first_hover = state
            .hover(&first_uri, Position::new(3, 7))
            .unwrap_or_else(|| panic!("first project hover should work: {publications:#?}"));
        let second_hover = state
            .hover(&second_uri, Position::new(3, 7))
            .expect("second project hover should work");

        assert_hover_contains(first_hover, "firstAnswer : Int");
        assert_hover_contains(second_hover, "secondAnswer : Int");

        drop(container);
    }

    #[test]
    fn solveig_lib_override_publishes_prelude_diagnostics() {
        if std::env::var_os(BROKEN_PRELUDE_CHILD).is_some() {
            let project_root = PathBuf::from(
                std::env::var_os(BROKEN_PRELUDE_PROJECT)
                    .expect("child project path should be present"),
            );
            let list = PathBuf::from(
                std::env::var_os(solveig_package::LIBRARY_ENV_VAR)
                    .expect("child library path should be present"),
            )
            .join("prelude/src/prelude/list.sv");
            let list = fs::canonicalize(list).expect("Prelude fixture path should canonicalize");
            let list_uri = Url::from_file_path(&list).expect("Prelude URI should be valid");
            let mut state = ServerState::new(PositionEncoding::Utf16);
            state.set_roots(vec![project_root.clone()]);
            state.touch_path(&project_root.join("src/main.sv"));

            let publications = state.check_all();
            let diagnostics = diagnostics_for_uri(&publications, &list_uri);

            assert!(
                !diagnostics.is_empty(),
                "broken Prelude diagnostics should publish at {list_uri}: {publications:#?}"
            );
            return;
        }

        let fixture = temp_root("broken_prelude");
        let library = fixture.join("lib");
        let project = fixture.join("project");
        write_broken_library(&library);
        write_source(
            &project.join("solveig.toml"),
            r#"[package]
name = "broken_prelude_client"
version = "0.1.0"

[language]
entry = "main"
"#,
        );
        write_source(&project.join("src/main.sv"), "main : Int\nmain = 1\n");

        let status = Command::new(std::env::current_exe().expect("test executable should resolve"))
            .arg("--exact")
            .arg("workspace::tests::solveig_lib_override_publishes_prelude_diagnostics")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(BROKEN_PRELUDE_CHILD, "1")
            .env(BROKEN_PRELUDE_PROJECT, &project)
            .env(solveig_package::LIBRARY_ENV_VAR, &library)
            .status()
            .expect("broken Prelude child test should start");

        assert!(status.success(), "broken Prelude child test should pass");
        drop(fixture);
    }

    fn assert_hover_contains(hover: Hover, needle: &str) {
        let HoverContents::Markup(markup) = hover.contents else {
            panic!("hover should use markup content");
        };

        assert!(
            markup.value.contains(needle),
            "expected hover `{}` to contain `{needle}`",
            markup.value
        );
    }

    fn diagnostics_for_uri(publications: &[(Url, Vec<Diagnostic>)], uri: &Url) -> Vec<Diagnostic> {
        publications
            .iter()
            .find_map(|(published_uri, diagnostics)| {
                (published_uri == uri).then(|| diagnostics.clone())
            })
            .unwrap_or_default()
    }

    #[derive(Clone, Copy, Debug)]
    struct RetainedLspCounts {
        workspaces: usize,
        open_files: usize,
        published_snapshots: usize,
        source_map_files: usize,
        snapshot_source_files: usize,
    }

    fn retained_lsp_counts(state: &ServerState, uri: &Url) -> RetainedLspCounts {
        let path = uri
            .to_file_path()
            .expect("test URI should convert to a file path");
        let root = state.workspace_root_for_path(&path);
        let workspace = state
            .workspaces
            .get(&root)
            .unwrap_or_else(|| panic!("missing workspace for {}", path.display()));
        let workspace = lock_workspace(workspace);
        let snapshot = state
            .published_snapshots
            .get(&root)
            .unwrap_or_else(|| panic!("missing published snapshot for {}", root.display()));

        RetainedLspCounts {
            workspaces: state.workspaces.len(),
            open_files: workspace.open_files.len(),
            published_snapshots: state.published_snapshots.len(),
            source_map_files: snapshot.source_map.files().len(),
            snapshot_source_files: snapshot.snapshot.source_files.len(),
        }
    }

    fn diagnostic_keys(
        diagnostics: &BTreeMap<Url, Vec<Diagnostic>>,
    ) -> BTreeSet<(String, String, u32, u32, u32, u32)> {
        let mut out = BTreeSet::new();

        for (uri, diagnostics) in diagnostics {
            for diagnostic in diagnostics {
                let code = diagnostic.code.as_ref().map_or_else(
                    || "<none>".to_string(),
                    |code| match code {
                        NumberOrString::Number(number) => number.to_string(),
                        NumberOrString::String(code) => code.clone(),
                    },
                );
                out.insert((
                    uri.to_string(),
                    code,
                    diagnostic.range.start.line,
                    diagnostic.range.start.character,
                    diagnostic.range.end.line,
                    diagnostic.range.end.character,
                ));
            }
        }

        out
    }
}
