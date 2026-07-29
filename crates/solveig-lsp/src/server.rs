//! Tower-LSP server lifecycle and text-document synchronization.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use lsp_types::{
    ClientCapabilities, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentSymbolParams, DocumentSymbolResponse, FileChangeType, GotoDefinitionParams,
    GotoDefinitionResponse, HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, InitializedParams, Location, MessageType, OneOf, PositionEncodingKind,
    ReferenceParams, ServerCapabilities, SymbolInformation, TextDocumentSyncCapability,
    TextDocumentSyncKind, WorkspaceSymbolParams,
};
use solveig_base::PositionEncoding;
use tokio::io::{stdin, stdout};
use tokio::sync::Mutex;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::workspace::ServerState;

const CHECK_DEBOUNCE: Duration = Duration::from_millis(150);

/// Serves Solveig LSP requests over stdin/stdout.
pub async fn run_stdio() {
    let (service, socket) = LspService::new(LspServer::new);
    Server::new(stdin(), stdout(), socket).serve(service).await;
}

/// Runs the stdio server on a current-thread Tokio runtime.
pub fn run_stdio_blocking() -> std::io::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run_stdio());
    Ok(())
}

pub struct LspServer {
    client: Client,
    state: Arc<Mutex<ServerState>>,
    revision: Arc<AtomicU64>,
}

impl LspServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(Mutex::new(ServerState::new(PositionEncoding::default()))),
            revision: Arc::new(AtomicU64::new(0)),
        }
    }

    fn bump_revision(&self) -> u64 {
        self.revision.fetch_add(1, Ordering::SeqCst) + 1
    }

    async fn schedule_check(&self, revision: u64) {
        let mut state = self.state.lock().await;
        if let Some(handle) = state.pending_check.take() {
            handle.abort();
        }

        let state_ref = Arc::clone(&self.state);
        let revision_ref = Arc::clone(&self.revision);
        let client = self.client.clone();
        state.pending_check = Some(tokio::spawn(async move {
            tokio::time::sleep(CHECK_DEBOUNCE).await;
            if revision_ref.load(Ordering::SeqCst) != revision {
                return;
            }

            let publications = {
                let mut state = state_ref.lock().await;
                let publications = state.check_all();
                state.pending_check = None;
                publications
            };

            if revision_ref.load(Ordering::SeqCst) != revision {
                return;
            }

            for (uri, diagnostics) in publications {
                client.publish_diagnostics(uri, diagnostics, None).await;
            }
        }));
    }

    async fn check_after_change(&self) {
        let revision = self.bump_revision();
        self.schedule_check(revision).await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for LspServer {
    async fn initialize(
        &self,
        params: InitializeParams,
    ) -> tower_lsp::jsonrpc::Result<InitializeResult> {
        let (encoding, lsp_encoding) = negotiate_position_encoding(&params.capabilities);
        let roots = workspace_roots(&params);
        {
            let mut state = self.state.lock().await;
            *state = ServerState::new(encoding);
            state.set_roots(roots);
        }

        Ok(InitializeResult {
            capabilities: server_capabilities(lsp_encoding),
            server_info: None,
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Solveig language server initialized")
            .await;
    }

    async fn shutdown(&self) -> tower_lsp::jsonrpc::Result<()> {
        let mut state = self.state.lock().await;
        if let Some(handle) = state.pending_check.take() {
            handle.abort();
        }
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        {
            let mut state = self.state.lock().await;
            state.open_document(&params.text_document.uri, params.text_document.text);
        }
        self.check_after_change().await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        let Some(change) = params.content_changes.pop() else {
            return;
        };

        {
            let mut state = self.state.lock().await;
            state.change_document(&params.text_document.uri, change.text);
        }
        self.check_after_change().await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        {
            let mut state = self.state.lock().await;
            state.save_document(&params.text_document.uri, params.text);
        }
        self.check_after_change().await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        {
            let mut state = self.state.lock().await;
            state.close_document(&params.text_document.uri);
        }
        self.check_after_change().await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        {
            let mut state = self.state.lock().await;
            for change in params.changes {
                if matches!(
                    change.typ,
                    FileChangeType::CREATED | FileChangeType::CHANGED | FileChangeType::DELETED
                ) && let Ok(path) = change.uri.to_file_path()
                {
                    state.touch_path(&path);
                }
            }
        }
        self.check_after_change().await;
    }

    async fn hover(
        &self,
        params: HoverParams,
    ) -> tower_lsp::jsonrpc::Result<Option<lsp_types::Hover>> {
        let mut state = self.state.lock().await;
        let params = params.text_document_position_params;

        Ok(state.hover(&params.text_document.uri, params.position))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> tower_lsp::jsonrpc::Result<Option<GotoDefinitionResponse>> {
        let mut state = self.state.lock().await;
        let params = params.text_document_position_params;

        Ok(state
            .definition(&params.text_document.uri, params.position)
            .map(GotoDefinitionResponse::Scalar))
    }

    async fn references(
        &self,
        params: ReferenceParams,
    ) -> tower_lsp::jsonrpc::Result<Option<Vec<Location>>> {
        let mut state = self.state.lock().await;
        let position = params.text_document_position.position;
        let uri = params.text_document_position.text_document.uri;

        Ok(state.references(&uri, position, params.context.include_declaration))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> tower_lsp::jsonrpc::Result<Option<DocumentSymbolResponse>> {
        let mut state = self.state.lock().await;

        Ok(state
            .document_symbols(&params.text_document.uri)
            .map(DocumentSymbolResponse::Nested))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> tower_lsp::jsonrpc::Result<Option<Vec<SymbolInformation>>> {
        let mut state = self.state.lock().await;

        Ok(Some(state.workspace_symbols(&params.query)))
    }
}

fn server_capabilities(encoding: PositionEncodingKind) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(encoding),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        ..ServerCapabilities::default()
    }
}

fn negotiate_position_encoding(
    capabilities: &ClientCapabilities,
) -> (PositionEncoding, PositionEncodingKind) {
    let encodings = capabilities
        .general
        .as_ref()
        .and_then(|general| general.position_encodings.as_ref());

    if encodings.is_some_and(|encodings| {
        encodings
            .iter()
            .any(|encoding| encoding == &PositionEncodingKind::UTF8)
    }) {
        (PositionEncoding::Utf8, PositionEncodingKind::UTF8)
    } else {
        (PositionEncoding::Utf16, PositionEncodingKind::UTF16)
    }
}

fn workspace_roots(params: &InitializeParams) -> Vec<PathBuf> {
    if let Some(folders) = &params.workspace_folders {
        let mut roots = folders
            .iter()
            .filter_map(|folder| folder.uri.to_file_path().ok())
            .collect::<Vec<_>>();
        roots.sort();
        roots.dedup();
        return roots;
    }

    params
        .root_uri
        .as_ref()
        .and_then(|uri| uri.to_file_path().ok())
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{
        DocumentSymbolParams, DocumentSymbolResponse, GeneralClientCapabilities,
        GotoDefinitionParams, GotoDefinitionResponse, HoverContents, PartialResultParams, Position,
        ReferenceContext, ReferenceParams, TextDocumentContentChangeEvent, TextDocumentIdentifier,
        TextDocumentItem, TextDocumentPositionParams, Url, VersionedTextDocumentIdentifier,
        WorkDoneProgressParams, WorkspaceFolder, WorkspaceSymbolParams,
    };
    use std::fs;
    use std::path::Path;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

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
    fn initialize_capabilities_use_full_sync_and_negotiate_utf8() {
        let capabilities = ClientCapabilities {
            general: Some(GeneralClientCapabilities {
                position_encodings: Some(vec![PositionEncodingKind::UTF8]),
                ..GeneralClientCapabilities::default()
            }),
            ..ClientCapabilities::default()
        };
        let (encoding, lsp_encoding) = negotiate_position_encoding(&capabilities);
        let capabilities = server_capabilities(lsp_encoding);

        assert_eq!(encoding, PositionEncoding::Utf8);
        assert_eq!(
            capabilities.text_document_sync,
            Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL))
        );
        assert_eq!(
            capabilities.position_encoding,
            Some(PositionEncodingKind::UTF8)
        );
        assert_eq!(
            capabilities.hover_provider,
            Some(HoverProviderCapability::Simple(true))
        );
        assert_eq!(capabilities.definition_provider, Some(OneOf::Left(true)));
        assert_eq!(capabilities.references_provider, Some(OneOf::Left(true)));
        assert_eq!(
            capabilities.document_symbol_provider,
            Some(OneOf::Left(true))
        );
        assert_eq!(
            capabilities.workspace_symbol_provider,
            Some(OneOf::Left(true))
        );
    }

    #[test]
    fn initialize_roots_use_workspace_folders_before_root_uri() {
        let first = Url::from_file_path("/tmp/solveig-first").expect("test URI should be valid");
        let second = Url::from_file_path("/tmp/solveig-second").expect("test URI should be valid");
        let params = InitializeParams {
            root_uri: Some(first),
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: second,
                name: "second".to_string(),
            }]),
            ..InitializeParams::default()
        };

        assert_eq!(
            workspace_roots(&params),
            vec![PathBuf::from("/tmp/solveig-second")]
        );
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn document_lifecycle_runs_debounced_diagnostics() {
        let project_root = temp_root("server_lifecycle");
        let source = project_root.join("scratch.sv");
        write_source(
            &source,
            r#"main : Int
main = True
"#,
        );
        let uri = Url::from_file_path(&source).expect("test path should become a file URI");
        let root_uri =
            Url::from_directory_path(&project_root).expect("test root should become a file URI");
        let (service, _socket) = LspService::new(LspServer::new);
        let server = service.inner();

        server
            .initialize(InitializeParams {
                root_uri: Some(root_uri),
                capabilities: ClientCapabilities::default(),
                ..InitializeParams::default()
            })
            .await
            .expect("initialize should succeed");

        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "solveig".to_string(),
                    version: 1,
                    text: r#"main : Int
main = True
"#
                    .to_string(),
                },
            })
            .await;

        {
            let state = server.state.lock().await;
            assert!(state.pending_check.is_some());
        }

        tokio::task::yield_now().await;
        tokio::time::advance(CHECK_DEBOUNCE).await;
        tokio::task::yield_now().await;

        {
            let state = server.state.lock().await;
            assert!(state.pending_check.is_none());
            assert_eq!(state.previous_diagnostic_count(), 1);
        }

        server
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: r#"main : Int
main = 1
"#
                    .to_string(),
                }],
            })
            .await;

        tokio::task::yield_now().await;
        tokio::time::advance(CHECK_DEBOUNCE).await;
        tokio::task::yield_now().await;

        {
            let state = server.state.lock().await;
            assert!(state.pending_check.is_none());
            assert_eq!(state.previous_diagnostic_count(), 0);
        }

        server.shutdown().await.expect("shutdown should succeed");
        let _ = fs::remove_dir_all(project_root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn text_document_hover_returns_markdown_signature_and_docs() {
        let project_root = temp_root("server_hover");
        let source = project_root.join("scratch.sv");
        write_source(
            &source,
            r#"-- | The answer used by hover.
answer : Int
answer = 42

main : Int
main = answer
"#,
        );
        let uri = Url::from_file_path(&source).expect("test path should become a file URI");
        let root_uri =
            Url::from_directory_path(&project_root).expect("test root should become a file URI");
        let (service, _socket) = LspService::new(LspServer::new);
        let server = service.inner();

        server
            .initialize(InitializeParams {
                root_uri: Some(root_uri),
                capabilities: ClientCapabilities::default(),
                ..InitializeParams::default()
            })
            .await
            .expect("initialize should succeed");

        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "solveig".to_string(),
                    version: 1,
                    text: r#"-- | The answer used by hover.
answer : Int
answer = 42

main : Int
main = answer
"#
                    .to_string(),
                },
            })
            .await;

        let hover = server
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(1, 0),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .expect("hover request should succeed")
            .expect("hover response should exist");

        let HoverContents::Markup(markup) = hover.contents else {
            panic!("hover should use markdown markup content");
        };

        assert!(markup.value.contains("```solveig\nanswer : Int\n```"));
        assert!(markup.value.contains("The answer used by hover."));
        assert_eq!(
            hover.range.expect("hover should carry a range").start.line,
            1
        );

        server.shutdown().await.expect("shutdown should succeed");
        let _ = fs::remove_dir_all(project_root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn text_document_navigation_returns_locations_and_symbols() {
        let project_root = temp_root("server_navigation");
        let main = project_root.join("src/main.sv");
        let lib = project_root.join("src/navLib.sv");
        let main_text = r#"import navLib(Box, MkBox, unwrap)

main : Int
main = unwrap (MkBox 41)
"#;
        write_source(
            &lib,
            r#"data Box = MkBox Int

unwrap : Box -> Int
unwrap value =
    case value of
        MkBox n => n
"#,
        );
        write_source(&main, main_text);
        let main_uri = Url::from_file_path(&main).expect("test path should become a file URI");
        let lib_uri = Url::from_file_path(&lib).expect("test path should become a file URI");
        let root_uri =
            Url::from_directory_path(&project_root).expect("test root should become a file URI");
        let (service, _socket) = LspService::new(LspServer::new);
        let server = service.inner();

        server
            .initialize(InitializeParams {
                root_uri: Some(root_uri),
                capabilities: ClientCapabilities::default(),
                ..InitializeParams::default()
            })
            .await
            .expect("initialize should succeed");

        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: main_uri.clone(),
                    language_id: "solveig".to_string(),
                    version: 1,
                    text: main_text.to_string(),
                },
            })
            .await;

        let definition = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: main_uri.clone(),
                    },
                    position: Position::new(3, 7),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .expect("definition request should succeed")
            .expect("definition response should exist");
        let GotoDefinitionResponse::Scalar(location) = definition else {
            panic!("definition should return one location");
        };

        assert_eq!(location.uri, lib_uri);
        assert_eq!(location.range.start.line, 2);
        assert_eq!(location.range.start.character, 0);

        let references = server
            .references(ReferenceParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: main_uri.clone(),
                    },
                    position: Position::new(3, 7),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: ReferenceContext {
                    include_declaration: true,
                },
            })
            .await
            .expect("references request should succeed")
            .expect("references response should exist");

        assert!(references.len() >= 3);
        assert!(references.iter().any(|location| location.uri == main_uri));
        assert!(references.iter().any(|location| location.uri == lib_uri));

        let symbols = server
            .document_symbol(DocumentSymbolParams {
                text_document: TextDocumentIdentifier {
                    uri: lib_uri.clone(),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .expect("document symbol request should succeed")
            .expect("document symbol response should exist");
        let DocumentSymbolResponse::Nested(symbols) = symbols else {
            panic!("document symbols should be hierarchical");
        };

        assert!(symbols.iter().any(|symbol| symbol.name == "unwrap"));
        assert!(symbols.iter().any(|symbol| {
            symbol.name == "Box"
                && symbol
                    .children
                    .as_ref()
                    .is_some_and(|children| children.iter().any(|child| child.name == "MkBox"))
        }));

        let workspace_symbols = server
            .symbol(WorkspaceSymbolParams {
                query: "box".to_string(),
                ..WorkspaceSymbolParams::default()
            })
            .await
            .expect("workspace symbol request should succeed")
            .expect("workspace symbol response should exist");
        let names = workspace_symbols
            .iter()
            .map(|symbol| symbol.name.as_str())
            .collect::<Vec<_>>();

        assert!(names.contains(&"Box"));
        assert!(names.contains(&"MkBox"));

        server.shutdown().await.expect("shutdown should succeed");
        let _ = fs::remove_dir_all(project_root);
    }
}
