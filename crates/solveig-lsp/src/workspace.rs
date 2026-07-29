//! Workspace state and synchronous checks behind the async language-server lock.

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use lsp_types::{
    CodeActionResponse, CompletionResponse, Diagnostic, DocumentSymbol, Hover, InlayHint, Location,
    Position, Range, SemanticToken, SemanticTokens, SemanticTokensDelta, SemanticTokensEdit,
    SemanticTokensFullDeltaResult, SymbolInformation, Url,
};
use solveig_base::{PositionEncoding, SourcePosition};
use solveig_driver::{Compilation, DriverError, SourceCodeActionCategory, SourceInlayHintConfig};
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
    workspaces: BTreeMap<PathBuf, Workspace>,
    roots: Vec<PathBuf>,
    previous_diagnostics: BTreeSet<Url>,
    semantic_token_results: BTreeMap<String, Vec<SemanticToken>>,
    next_semantic_token_result: u64,
    pub(crate) pending_check: Option<JoinHandle<()>>,
    encoding: PositionEncoding,
    config: ServerConfig,
}

pub(crate) struct Workspace {
    compilation: Compilation,
    open_files: BTreeSet<PathBuf>,
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
        Self {
            workspaces: BTreeMap::new(),
            roots: Vec::new(),
            previous_diagnostics: BTreeSet::new(),
            semantic_token_results: BTreeMap::new(),
            next_semantic_token_result: 0,
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
        workspace.open_files.insert(path.clone());
        workspace.compilation.set_overlay(path, text);
        Some(())
    }

    pub(crate) fn change_document(&mut self, uri: &Url, text: String) -> Option<()> {
        let path = uri.to_file_path().ok()?;
        self.workspace_for_path(&path)
            .compilation
            .set_overlay(path, text);
        Some(())
    }

    pub(crate) fn save_document(&mut self, uri: &Url, text: Option<String>) -> Option<()> {
        let path = uri.to_file_path().ok()?;
        if let Some(text) = text {
            self.workspace_for_path(&path)
                .compilation
                .set_overlay(path, text);
        } else {
            self.workspace_for_path(&path);
        }
        Some(())
    }

    pub(crate) fn close_document(&mut self, uri: &Url) -> Option<()> {
        let path = uri.to_file_path().ok()?;
        let workspace = self.workspace_for_path(&path);
        workspace.open_files.remove(&path);
        workspace.compilation.clear_overlay(path);
        Some(())
    }

    pub(crate) fn touch_path(&mut self, path: &Path) {
        self.workspace_for_path(path);
    }

    pub(crate) fn check_all(&mut self) -> Vec<(Url, Vec<Diagnostic>)> {
        let mut current = BTreeMap::<Url, Vec<Diagnostic>>::new();

        for workspace in self.workspaces.values_mut() {
            current.extend(workspace.check(self.encoding, self.config));
        }

        let mut publications = Vec::new();
        let current_uris = current.keys().cloned().collect::<BTreeSet<_>>();

        for (uri, diagnostics) in current {
            publications.push((uri, diagnostics));
        }

        for uri in self.previous_diagnostics.difference(&current_uris) {
            publications.push((uri.clone(), Vec::new()));
        }

        self.previous_diagnostics = current_uris;
        publications
    }

    pub(crate) fn hover(&mut self, uri: &Url, position: Position) -> Option<Hover> {
        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;

        self.workspace_for_path(&path)
            .hover(&path, position, encoding)
    }

    pub(crate) fn definition(&mut self, uri: &Url, position: Position) -> Option<Location> {
        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;

        self.workspace_for_path(&path)
            .definition(&path, position, encoding)
    }

    pub(crate) fn references(
        &mut self,
        uri: &Url,
        position: Position,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;

        self.workspace_for_path(&path)
            .references(&path, position, encoding, include_declaration)
    }

    pub(crate) fn completion(
        &mut self,
        uri: &Url,
        position: Position,
    ) -> Option<CompletionResponse> {
        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;

        self.workspace_for_path(&path)
            .completion(&path, position, encoding)
    }

    pub(crate) fn inlay_hints(&mut self, uri: &Url, range: Range) -> Option<Vec<InlayHint>> {
        if !self.config.inlay_hints_enabled {
            return Some(Vec::new());
        }

        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;

        self.workspace_for_path(&path)
            .inlay_hints(&path, range, encoding)
    }

    pub(crate) fn document_symbols(&mut self, uri: &Url) -> Option<Vec<DocumentSymbol>> {
        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;

        self.workspace_for_path(&path)
            .document_symbols(&path, encoding)
    }

    pub(crate) fn workspace_symbols(&mut self, query: &str) -> Vec<SymbolInformation> {
        let encoding = self.encoding;
        let mut out = Vec::new();

        for workspace in self.workspaces.values_mut() {
            out.extend(workspace.workspace_symbols(query, encoding));
        }

        out
    }

    pub(crate) fn semantic_tokens_full(&mut self, uri: &Url) -> Option<SemanticTokens> {
        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;
        let tokens = self
            .workspace_for_path(&path)
            .semantic_tokens(&path, encoding)?;

        Some(self.remember_semantic_tokens(tokens))
    }

    pub(crate) fn semantic_tokens_delta(
        &mut self,
        uri: &Url,
        previous_result_id: &str,
    ) -> Option<SemanticTokensFullDeltaResult> {
        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;
        let tokens = self
            .workspace_for_path(&path)
            .semantic_tokens(&path, encoding)?;
        let previous = self.semantic_token_results.get(previous_result_id).cloned();
        let result = self.remember_semantic_tokens(tokens.clone());

        Some(match previous {
            Some(previous) => SemanticTokensFullDeltaResult::TokensDelta(SemanticTokensDelta {
                result_id: result.result_id,
                edits: vec![SemanticTokensEdit {
                    start: 0,
                    delete_count: semantic_token_u32_len(&previous),
                    data: Some(tokens),
                }],
            }),
            None => SemanticTokensFullDeltaResult::Tokens(result),
        })
    }

    pub(crate) fn code_actions(&mut self, uri: &Url, range: Range) -> Option<CodeActionResponse> {
        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;
        let include_inferred_signatures = self.config.inferred_signature_hints_enabled;

        self.workspace_for_path(&path).code_actions(
            &path,
            range,
            encoding,
            include_inferred_signatures,
        )
    }

    #[cfg(test)]
    pub(crate) fn previous_diagnostic_count(&self) -> usize {
        self.previous_diagnostics.len()
    }

    fn workspace_for_path(&mut self, path: &Path) -> &mut Workspace {
        let root = self.workspace_root_for_path(path);

        self.workspaces
            .entry(root)
            .or_insert_with_key(|root| Workspace::new(root, path))
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

    fn remember_semantic_tokens(&mut self, tokens: Vec<SemanticToken>) -> SemanticTokens {
        self.next_semantic_token_result = self.next_semantic_token_result.saturating_add(1);
        let result_id = format!("solveig-semantic-{}", self.next_semantic_token_result);
        self.semantic_token_results
            .insert(result_id.clone(), tokens.clone());

        SemanticTokens {
            result_id: Some(result_id),
            data: tokens,
        }
    }
}

impl Workspace {
    fn new(root: &Path, first_file: &Path) -> Self {
        let compilation = if root.join("src").join("main.sv").is_file() {
            Compilation::new(root)
        } else {
            Compilation::for_source_file(first_file)
        };

        Self {
            compilation,
            open_files: BTreeSet::new(),
        }
    }

    fn check(
        &mut self,
        encoding: PositionEncoding,
        config: ServerConfig,
    ) -> BTreeMap<Url, Vec<Diagnostic>> {
        let checked = catch_unwind(AssertUnwindSafe(|| {
            let mut diagnostics = {
                let result = self.compilation.check(true)?;
                diagnostic_map(result.diagnostics(), result.source_map(), encoding)
            };

            if config.inferred_signature_hints_enabled {
                extend_diagnostic_map(
                    &mut diagnostics,
                    inferred_signature_diagnostic_map(
                        self.compilation.source_code_actions(),
                        self.compilation.source_map(),
                        encoding,
                    ),
                );
            }

            Ok(diagnostics)
        }));

        match checked {
            Ok(Ok(diagnostics)) => diagnostics,
            Ok(Err(DriverError::EntryFileMissing { path })) => {
                missing_entry_diagnostic(&path).into_iter().collect()
            }
            Ok(Err(_)) | Err(_) => BTreeMap::new(),
        }
    }

    fn hover(
        &mut self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
    ) -> Option<Hover> {
        catch_unwind(AssertUnwindSafe(|| {
            let info = self.compilation.hover_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
            )?;

            Some(lsp_hover(&info, self.compilation.source_map(), encoding))
        }))
        .ok()
        .flatten()
    }

    fn definition(
        &mut self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
    ) -> Option<Location> {
        catch_unwind(AssertUnwindSafe(|| {
            let span = self.compilation.definition_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
            )?;

            lsp_location(span, self.compilation.source_map(), encoding)
        }))
        .ok()
        .flatten()
    }

    fn references(
        &mut self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
        catch_unwind(AssertUnwindSafe(|| {
            let spans = self.compilation.references_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
                include_declaration,
            )?;

            Some(
                spans
                    .into_iter()
                    .filter_map(|span| lsp_location(span, self.compilation.source_map(), encoding))
                    .collect(),
            )
        }))
        .ok()
        .flatten()
    }

    fn completion(
        &mut self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
    ) -> Option<CompletionResponse> {
        catch_unwind(AssertUnwindSafe(|| {
            let items = self.compilation.completion_items_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
            );

            Some(CompletionResponse::Array(lsp_completion_items(&items)))
        }))
        .ok()
        .flatten()
    }

    fn inlay_hints(
        &mut self,
        path: &Path,
        range: Range,
        encoding: PositionEncoding,
    ) -> Option<Vec<InlayHint>> {
        catch_unwind(AssertUnwindSafe(|| {
            let hints = self.compilation.inlay_hints_at_range(
                path,
                SourcePosition::new(range.start.line, range.start.character),
                SourcePosition::new(range.end.line, range.end.character),
                encoding,
                SourceInlayHintConfig::default(),
            );

            Some(lsp_inlay_hints(
                &hints,
                self.compilation.source_map(),
                encoding,
            ))
        }))
        .ok()
        .flatten()
    }

    fn document_symbols(
        &mut self,
        path: &Path,
        encoding: PositionEncoding,
    ) -> Option<Vec<DocumentSymbol>> {
        catch_unwind(AssertUnwindSafe(|| {
            let symbols = self.compilation.document_symbols(path);
            Some(lsp_document_symbols(
                &symbols,
                self.compilation.source_map(),
                encoding,
            ))
        }))
        .ok()
        .flatten()
    }

    fn workspace_symbols(
        &mut self,
        query: &str,
        encoding: PositionEncoding,
    ) -> Vec<SymbolInformation> {
        catch_unwind(AssertUnwindSafe(|| {
            let symbols = self.compilation.workspace_symbols(query);
            lsp_symbol_information(&symbols, self.compilation.source_map(), encoding)
        }))
        .unwrap_or_default()
    }

    fn semantic_tokens(
        &mut self,
        path: &Path,
        encoding: PositionEncoding,
    ) -> Option<Vec<SemanticToken>> {
        catch_unwind(AssertUnwindSafe(|| {
            let tokens = self.compilation.semantic_tokens(path);
            Some(lsp_semantic_tokens(
                &tokens,
                self.compilation.source_map(),
                encoding,
            ))
        }))
        .ok()
        .flatten()
    }

    fn code_actions(
        &mut self,
        path: &Path,
        range: Range,
        encoding: PositionEncoding,
        include_inferred_signatures: bool,
    ) -> Option<CodeActionResponse> {
        catch_unwind(AssertUnwindSafe(|| {
            let mut actions = self.compilation.code_actions_at_range(
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

            Some(lsp_code_actions(
                &actions,
                self.compilation.source_map(),
                encoding,
            ))
        }))
        .ok()
        .flatten()
    }
}

fn semantic_token_u32_len(tokens: &[SemanticToken]) -> u32 {
    tokens
        .len()
        .saturating_mul(5)
        .try_into()
        .unwrap_or(u32::MAX)
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

    for candidate in source_dir.ancestors() {
        if candidate.join("src").join("main.sv").is_file() {
            return candidate.to_path_buf();
        }
    }

    source_dir.to_path_buf()
}

fn is_solveig_project_root(path: &Path) -> bool {
    path.join("src").join("main.sv").is_file()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::diagnostics::INFERRED_SIGNATURE_DIAGNOSTIC_CODE;
    use lsp_types::{CodeActionOrCommand, DiagnosticSeverity, HoverContents, NumberOrString};
    use solveig_driver::Compilation;
    use std::fs;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

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

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("solveig_lsp_{name}_{}_{}", process::id(), nanos))
    }

    fn write_source(path: &Path, source: &str) {
        fs::create_dir_all(
            path.parent()
                .expect("test source should have a parent directory"),
        )
        .expect("test source directory should be creatable");
        fs::write(path, source).expect("test source should be writable");
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
                .check(true)
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

        let _ = fs::remove_dir_all(project_root);
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

        let _ = fs::remove_dir_all(project_root);
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
        state.set_roots(vec![container.clone()]);

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
        assert!(!state.workspaces.contains_key(&container));

        let first_hover = state
            .hover(&first_uri, Position::new(3, 7))
            .expect("first project hover should work");
        let second_hover = state
            .hover(&second_uri, Position::new(3, 7))
            .expect("second project hover should work");

        assert_hover_contains(first_hover, "firstAnswer : Int");
        assert_hover_contains(second_hover, "secondAnswer : Int");

        let _ = fs::remove_dir_all(container);
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
