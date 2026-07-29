//! Workspace state and synchronous checks behind the async language-server lock.

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use lsp_types::{Diagnostic, DocumentSymbol, Hover, Location, Position, SymbolInformation, Url};
use solveig_base::{PositionEncoding, SourcePosition};
use solveig_driver::{Compilation, DriverError};
use tokio::task::JoinHandle;

use crate::diagnostics::{diagnostic_map, missing_entry_diagnostic};
use crate::hover::lsp_hover;
use crate::navigation::{lsp_document_symbols, lsp_location, lsp_symbol_information};

pub(crate) struct ServerState {
    workspaces: BTreeMap<PathBuf, Workspace>,
    roots: Vec<PathBuf>,
    previous_diagnostics: BTreeSet<Url>,
    pub(crate) pending_check: Option<JoinHandle<()>>,
    encoding: PositionEncoding,
}

pub(crate) struct Workspace {
    compilation: Compilation,
    open_files: BTreeSet<PathBuf>,
}

impl ServerState {
    pub(crate) fn new(encoding: PositionEncoding) -> Self {
        Self {
            workspaces: BTreeMap::new(),
            roots: Vec::new(),
            previous_diagnostics: BTreeSet::new(),
            pending_check: None,
            encoding,
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
            current.extend(workspace.check(self.encoding));
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

    #[cfg(test)]
    pub(crate) fn previous_diagnostic_count(&self) -> usize {
        self.previous_diagnostics.len()
    }

    fn workspace_for_path(&mut self, path: &Path) -> &mut Workspace {
        let root = self
            .roots
            .iter()
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())
            .cloned()
            .unwrap_or_else(|| project_root_for_file(path));

        self.workspaces
            .entry(root)
            .or_insert_with_key(|root| Workspace::new(root, path))
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

    fn check(&mut self, encoding: PositionEncoding) -> BTreeMap<Url, Vec<Diagnostic>> {
        let checked = catch_unwind(AssertUnwindSafe(|| {
            self.compilation
                .check(true)
                .map(|result| diagnostic_map(result.diagnostics(), result.source_map(), encoding))
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use lsp_types::NumberOrString;
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
