//! Tower-LSP server lifecycle and text-document synchronization.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use lsp_types::{
    ClientCapabilities, CodeActionKind, CodeActionOptions, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, CompletionOptions, CompletionParams,
    CompletionResponse, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentHighlight, DocumentHighlightParams, DocumentLink, DocumentLinkOptions,
    DocumentLinkParams, DocumentSymbolParams, DocumentSymbolResponse, FileChangeType, FoldingRange,
    FoldingRangeParams, FoldingRangeProviderCapability, GotoDefinitionParams,
    GotoDefinitionResponse, HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, InitializedParams, InlayHintOptions, InlayHintParams,
    InlayHintServerCapabilities, Location, MessageType, OneOf, PositionEncodingKind,
    PrepareRenameResponse, ReferenceParams, RenameOptions, RenameParams, SelectionRange,
    SelectionRangeParams, SelectionRangeProviderCapability, SemanticTokensFullDeltaResult,
    SemanticTokensFullOptions, SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, SignatureHelp, SignatureHelpOptions,
    SignatureHelpParams, SymbolInformation, TextDocumentSyncCapability, TextDocumentSyncKind,
    WorkspaceEdit, WorkspaceSymbolParams,
};
use solveig_base::PositionEncoding;
use tokio::io::{stdin, stdout};
use tokio::sync::Mutex;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::semantic_tokens::semantic_tokens_legend;
use crate::workspace::{ServerConfig, ServerState};

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
        let config = server_config_from_initialize(&params);
        {
            let mut state = self.state.lock().await;
            *state = ServerState::new_with_config(encoding, config);
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

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> tower_lsp::jsonrpc::Result<Option<Vec<DocumentHighlight>>> {
        let mut state = self.state.lock().await;
        let params = params.text_document_position_params;

        Ok(state.document_highlights(&params.text_document.uri, params.position))
    }

    async fn document_link(
        &self,
        params: DocumentLinkParams,
    ) -> tower_lsp::jsonrpc::Result<Option<Vec<DocumentLink>>> {
        let mut state = self.state.lock().await;

        Ok(state.document_links(&params.text_document.uri))
    }

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> tower_lsp::jsonrpc::Result<Option<CompletionResponse>> {
        let mut state = self.state.lock().await;
        let params = params.text_document_position;

        Ok(state.completion(&params.text_document.uri, params.position))
    }

    async fn inlay_hint(
        &self,
        params: InlayHintParams,
    ) -> tower_lsp::jsonrpc::Result<Option<Vec<lsp_types::InlayHint>>> {
        let mut state = self.state.lock().await;

        Ok(state.inlay_hints(&params.text_document.uri, params.range))
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> tower_lsp::jsonrpc::Result<Option<Vec<FoldingRange>>> {
        let mut state = self.state.lock().await;

        Ok(state.folding_ranges(&params.text_document.uri))
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> tower_lsp::jsonrpc::Result<Option<Vec<SelectionRange>>> {
        let mut state = self.state.lock().await;

        Ok(state.selection_ranges(&params.text_document.uri, params.positions))
    }

    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> tower_lsp::jsonrpc::Result<Option<SignatureHelp>> {
        let mut state = self.state.lock().await;
        let params = params.text_document_position_params;

        Ok(state.signature_help(&params.text_document.uri, params.position))
    }

    async fn prepare_rename(
        &self,
        params: lsp_types::TextDocumentPositionParams,
    ) -> tower_lsp::jsonrpc::Result<Option<PrepareRenameResponse>> {
        let mut state = self.state.lock().await;

        Ok(state.prepare_rename(&params.text_document.uri, params.position))
    }

    async fn rename(
        &self,
        params: RenameParams,
    ) -> tower_lsp::jsonrpc::Result<Option<WorkspaceEdit>> {
        let mut state = self.state.lock().await;
        let position = params.text_document_position.position;
        let uri = params.text_document_position.text_document.uri;

        Ok(state.rename(&uri, position, &params.new_name))
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

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> tower_lsp::jsonrpc::Result<Option<SemanticTokensResult>> {
        let mut state = self.state.lock().await;

        Ok(state
            .semantic_tokens_full(&params.text_document.uri)
            .map(SemanticTokensResult::Tokens))
    }

    async fn semantic_tokens_full_delta(
        &self,
        params: lsp_types::SemanticTokensDeltaParams,
    ) -> tower_lsp::jsonrpc::Result<Option<SemanticTokensFullDeltaResult>> {
        let mut state = self.state.lock().await;

        Ok(state.semantic_tokens_delta(&params.text_document.uri, &params.previous_result_id))
    }

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> tower_lsp::jsonrpc::Result<Option<CodeActionResponse>> {
        let mut state = self.state.lock().await;

        Ok(state.code_actions(&params.text_document.uri, params.range))
    }
}

fn server_config_from_initialize(params: &InitializeParams) -> ServerConfig {
    let mut config = ServerConfig::default();

    if let Some(value) = params
        .initialization_options
        .as_ref()
        .and_then(|options| options.get("inlayHints"))
        .and_then(|options| options.get("enabled"))
        .and_then(|enabled| enabled.as_bool())
    {
        config.inlay_hints_enabled = value;
    }

    if let Some(value) = params
        .initialization_options
        .as_ref()
        .and_then(|options| options.get("diagnostics"))
        .and_then(|options| options.get("inferredSignatures"))
        .and_then(|options| options.get("enabled"))
        .and_then(|enabled| enabled.as_bool())
    {
        config.inferred_signature_hints_enabled = value;
    }

    config
}

fn server_capabilities(encoding: PositionEncodingKind) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(encoding),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec![".".to_string()]),
            ..CompletionOptions::default()
        }),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec![" ".to_string(), "(".to_string()]),
            retrigger_characters: Some(vec![" ".to_string()]),
            work_done_progress_options: Default::default(),
        }),
        inlay_hint_provider: Some(OneOf::Right(InlayHintServerCapabilities::Options(
            InlayHintOptions {
                resolve_provider: Some(false),
                work_done_progress_options: Default::default(),
            },
        ))),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        document_link_provider: Some(DocumentLinkOptions {
            resolve_provider: Some(false),
            work_done_progress_options: Default::default(),
        }),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
            resolve_provider: Some(false),
            work_done_progress_options: Default::default(),
        })),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                work_done_progress_options: Default::default(),
                legend: semantic_tokens_legend(),
                range: None,
                full: Some(SemanticTokensFullOptions::Delta { delta: Some(true) }),
            },
        )),
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
mod tests;
