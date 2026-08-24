//! Async-LSP server lifecycle and text-document synchronization.

use std::future::Future;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use async_lsp::lsp_types::notification::{LogMessage, PublishDiagnostics};
use async_lsp::lsp_types::{
    ClientCapabilities, CodeActionKind, CodeActionOptions, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, CompletionOptions, CompletionParams,
    CompletionResponse, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentFormattingParams, DocumentHighlight, DocumentHighlightParams, DocumentLink,
    DocumentLinkOptions, DocumentLinkParams, DocumentRangeFormattingParams, DocumentSymbolParams,
    DocumentSymbolResponse, FileChangeType, FoldingRange, FoldingRangeParams,
    FoldingRangeProviderCapability, FormattingOptions, FormattingProperty, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, InitializedParams, InlayHint, InlayHintOptions, InlayHintParams,
    InlayHintServerCapabilities, Location, LogMessageParams, MessageType, OneOf,
    PositionEncodingKind, PrepareRenameResponse, PublishDiagnosticsParams, ReferenceParams,
    RenameOptions, RenameParams, SelectionRange, SelectionRangeParams,
    SelectionRangeProviderCapability, SemanticTokensFullOptions, SemanticTokensOptions,
    SemanticTokensParams, SemanticTokensResult, SemanticTokensServerCapabilities,
    ServerCapabilities, SignatureHelp, SignatureHelpOptions, SignatureHelpParams,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit,
    WorkspaceEdit, WorkspaceSymbolParams, WorkspaceSymbolResponse,
};
use async_lsp::router::Router;
use async_lsp::{ClientSocket, ErrorCode, LanguageServer, MainLoop, ResponseError};
use solveig_base::PositionEncoding;
use solveig_ide::SourceEditError;

use crate::semantic_tokens::semantic_tokens_legend;
use crate::workspace::{ServerConfig, ServerState, WorkspaceSnapshot, check_workspaces};

const CHECK_DEBOUNCE: Duration = Duration::from_millis(150);
const DEFAULT_FORMAT_WIDTH: usize = 100;

type LspResult<T> = Result<T, ResponseError>;
type LspFuture<T> = Pin<Box<dyn Future<Output = LspResult<T>> + Send + 'static>>;
type NotifyResult = ControlFlow<async_lsp::Result<()>>;

/// Serves Solveig LSP requests over stdin/stdout.
pub async fn run_stdio() -> async_lsp::Result<()> {
    let (server, _) = MainLoop::new_server(LspServer::router);
    let stdin = async_lsp::stdio::PipeStdin::lock_tokio()?;
    let stdout = async_lsp::stdio::PipeStdout::lock_tokio()?;

    server.run_buffered(stdin, stdout).await
}

/// Runs the stdio server on a current-thread Tokio runtime.
pub fn run_stdio_blocking() -> std::io::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime
        .block_on(run_stdio())
        .map_err(|err| std::io::Error::other(err.to_string()))
}

/// Adapts Solveig workspace state to async-lsp protocol requests.
pub struct LspServer {
    client: ClientSocket,
    state: Arc<Mutex<ServerState>>,
    revision: Arc<AtomicU64>,
}

impl LspServer {
    /// Creates a server with an LSP client socket.
    pub fn new(client: ClientSocket) -> Self {
        Self {
            client,
            state: Arc::new(Mutex::new(ServerState::new(PositionEncoding::default()))),
            revision: Arc::new(AtomicU64::new(0)),
        }
    }

    fn router(client: ClientSocket) -> Router<Self> {
        Router::from_language_server(Self::new(client))
    }

    fn bump_revision(&self) -> u64 {
        self.revision.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn schedule_check(&self, revision: u64) {
        let state_ref = Arc::clone(&self.state);
        let revision_ref = Arc::clone(&self.revision);
        let client = self.client.clone();

        let mut state = lock_state(&self.state);
        if let Some(handle) = state.pending_check.take() {
            handle.abort();
        }

        state.pending_check = Some(tokio::spawn(async move {
            tokio::time::sleep(CHECK_DEBOUNCE).await;
            if revision_ref.load(Ordering::SeqCst) != revision {
                return;
            }

            let (encoding, config, plan) = {
                let state = lock_state(&state_ref);
                state.check_context()
            };
            let outcomes = check_workspaces(plan, encoding, config);

            let publications = {
                let mut state = lock_state(&state_ref);
                if revision_ref.load(Ordering::SeqCst) != revision {
                    return;
                }

                state.pending_check = None;
                state.finish_check(outcomes)
            };

            publish_diagnostics(&client, publications);
        }));
    }

    fn check_after_change(&self) {
        let revision = self.bump_revision();
        self.schedule_check(revision);
    }

    fn log_message(&self, typ: MessageType, message: impl Into<String>) {
        let message = message.into();
        if let Err(err) = self
            .client
            .notify::<LogMessage>(LogMessageParams { typ, message })
        {
            tracing::debug!(error = %err, "Failed to send Solveig LSP log message");
        }
    }
}

impl LanguageServer for LspServer {
    type Error = ResponseError;
    type NotifyResult = NotifyResult;

    fn initialize(&mut self, params: InitializeParams) -> LspFuture<InitializeResult> {
        let (encoding, lsp_encoding) = negotiate_position_encoding(&params.capabilities);
        let roots = workspace_roots(&params);
        let config = server_config_from_initialize(&params);
        {
            let mut state = lock_state(&self.state);
            *state = ServerState::new_with_config(encoding, config);
            state.set_roots(roots);
        }

        ready_response(InitializeResult {
            capabilities: server_capabilities(lsp_encoding),
            server_info: None,
        })
    }

    fn initialized(&mut self, _: InitializedParams) -> NotifyResult {
        tracing::info!("Solveig language server initialized");
        self.log_message(MessageType::INFO, "Solveig language server initialized");
        ControlFlow::Continue(())
    }

    fn shutdown(&mut self, (): ()) -> LspFuture<()> {
        let mut state = lock_state(&self.state);
        if let Some(handle) = state.pending_check.take() {
            handle.abort();
        }

        ready_response(())
    }

    fn exit(&mut self, (): ()) -> NotifyResult {
        let mut state = lock_state(&self.state);
        if let Some(handle) = state.pending_check.take() {
            handle.abort();
        }

        ControlFlow::Break(Ok(()))
    }

    fn did_open(&mut self, params: DidOpenTextDocumentParams) -> NotifyResult {
        {
            let mut state = lock_state(&self.state);
            state.open_document(&params.text_document.uri, params.text_document.text);
        }
        self.check_after_change();

        ControlFlow::Continue(())
    }

    fn did_change(&mut self, mut params: DidChangeTextDocumentParams) -> NotifyResult {
        let Some(change) = params.content_changes.pop() else {
            return ControlFlow::Continue(());
        };

        {
            let mut state = lock_state(&self.state);
            state.change_document(&params.text_document.uri, change.text);
        }
        self.check_after_change();

        ControlFlow::Continue(())
    }

    fn did_save(&mut self, params: DidSaveTextDocumentParams) -> NotifyResult {
        {
            let mut state = lock_state(&self.state);
            state.save_document(&params.text_document.uri, params.text);
        }
        self.check_after_change();

        ControlFlow::Continue(())
    }

    fn did_close(&mut self, params: DidCloseTextDocumentParams) -> NotifyResult {
        {
            let mut state = lock_state(&self.state);
            state.close_document(&params.text_document.uri);
        }
        self.check_after_change();

        ControlFlow::Continue(())
    }

    fn did_change_watched_files(&mut self, params: DidChangeWatchedFilesParams) -> NotifyResult {
        {
            let mut state = lock_state(&self.state);
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
        self.check_after_change();

        ControlFlow::Continue(())
    }

    fn hover(&mut self, params: HoverParams) -> LspFuture<Option<Hover>> {
        let params = params.text_document_position_params;
        let position = params.position;

        snapshot_response(
            &self.state,
            &params.text_document.uri,
            move |path, encoding, _, snapshot| snapshot.hover(&path, position, encoding),
        )
    }

    fn definition(
        &mut self,
        params: GotoDefinitionParams,
    ) -> LspFuture<Option<GotoDefinitionResponse>> {
        let params = params.text_document_position_params;
        let position = params.position;

        snapshot_response(
            &self.state,
            &params.text_document.uri,
            move |path, encoding, _, snapshot| {
                snapshot
                    .definition(&path, position, encoding)
                    .map(GotoDefinitionResponse::Scalar)
            },
        )
    }

    fn references(&mut self, params: ReferenceParams) -> LspFuture<Option<Vec<Location>>> {
        let position = params.text_document_position.position;
        let uri = params.text_document_position.text_document.uri;
        let include_declaration = params.context.include_declaration;

        snapshot_response(&self.state, &uri, move |path, encoding, _, snapshot| {
            snapshot.references(&path, position, encoding, include_declaration)
        })
    }

    fn document_highlight(
        &mut self,
        params: DocumentHighlightParams,
    ) -> LspFuture<Option<Vec<DocumentHighlight>>> {
        let params = params.text_document_position_params;
        let position = params.position;

        snapshot_response(
            &self.state,
            &params.text_document.uri,
            move |path, encoding, _, snapshot| {
                snapshot.document_highlights(&path, position, encoding)
            },
        )
    }

    fn document_link(
        &mut self,
        params: DocumentLinkParams,
    ) -> LspFuture<Option<Vec<DocumentLink>>> {
        snapshot_response(
            &self.state,
            &params.text_document.uri,
            move |path, encoding, _, snapshot| snapshot.document_links(&path, encoding),
        )
    }

    fn formatting(&mut self, params: DocumentFormattingParams) -> LspFuture<Option<Vec<TextEdit>>> {
        let width = formatting_width(&params.options);

        snapshot_result_response(
            &self.state,
            &params.text_document.uri,
            move |path, encoding, _, snapshot| {
                snapshot
                    .format_document(&path, encoding, width)
                    .map_err(source_edit_response_error)
            },
        )
    }

    fn range_formatting(
        &mut self,
        params: DocumentRangeFormattingParams,
    ) -> LspFuture<Option<Vec<TextEdit>>> {
        let width = formatting_width(&params.options);

        snapshot_result_response(
            &self.state,
            &params.text_document.uri,
            move |path, encoding, _, snapshot| {
                snapshot
                    .format_range(&path, params.range, encoding, width)
                    .map_err(source_edit_response_error)
            },
        )
    }

    fn completion(&mut self, params: CompletionParams) -> LspFuture<Option<CompletionResponse>> {
        let params = params.text_document_position;
        let position = params.position;

        snapshot_response(
            &self.state,
            &params.text_document.uri,
            move |path, encoding, _, snapshot| snapshot.completion(&path, position, encoding),
        )
    }

    fn inlay_hint(&mut self, params: InlayHintParams) -> LspFuture<Option<Vec<InlayHint>>> {
        snapshot_response(
            &self.state,
            &params.text_document.uri,
            move |path, encoding, config, snapshot| {
                if !config.inlay_hints_enabled {
                    return Some(Vec::new());
                }

                snapshot.inlay_hints(&path, params.range, encoding)
            },
        )
    }

    fn folding_range(
        &mut self,
        params: FoldingRangeParams,
    ) -> LspFuture<Option<Vec<FoldingRange>>> {
        snapshot_response(
            &self.state,
            &params.text_document.uri,
            move |path, encoding, _, snapshot| snapshot.folding_ranges(&path, encoding),
        )
    }

    fn selection_range(
        &mut self,
        params: SelectionRangeParams,
    ) -> LspFuture<Option<Vec<SelectionRange>>> {
        snapshot_response(
            &self.state,
            &params.text_document.uri,
            move |path, encoding, _, snapshot| {
                snapshot.selection_ranges(&path, params.positions, encoding)
            },
        )
    }

    fn signature_help(&mut self, params: SignatureHelpParams) -> LspFuture<Option<SignatureHelp>> {
        let params = params.text_document_position_params;
        let position = params.position;

        snapshot_response(
            &self.state,
            &params.text_document.uri,
            move |path, encoding, _, snapshot| snapshot.signature_help(&path, position, encoding),
        )
    }

    fn prepare_rename(
        &mut self,
        params: TextDocumentPositionParams,
    ) -> LspFuture<Option<PrepareRenameResponse>> {
        let position = params.position;

        snapshot_result_response(
            &self.state,
            &params.text_document.uri,
            move |path, encoding, _, snapshot| {
                snapshot
                    .prepare_rename(&path, position, encoding)
                    .map_err(source_edit_response_error)
            },
        )
    }

    fn rename(&mut self, params: RenameParams) -> LspFuture<Option<WorkspaceEdit>> {
        let position = params.text_document_position.position;
        let uri = params.text_document_position.text_document.uri;
        let new_name = params.new_name;

        snapshot_result_response(&self.state, &uri, move |path, encoding, _, snapshot| {
            snapshot
                .rename(&path, position, encoding, &new_name)
                .map_err(source_edit_response_error)
        })
    }

    fn document_symbol(
        &mut self,
        params: DocumentSymbolParams,
    ) -> LspFuture<Option<DocumentSymbolResponse>> {
        snapshot_response(
            &self.state,
            &params.text_document.uri,
            move |path, encoding, _, snapshot| {
                snapshot
                    .document_symbols(&path, encoding)
                    .map(DocumentSymbolResponse::Nested)
            },
        )
    }

    fn symbol(
        &mut self,
        params: WorkspaceSymbolParams,
    ) -> LspFuture<Option<WorkspaceSymbolResponse>> {
        let (encoding, snapshots) = {
            let state = lock_state(&self.state);
            state.workspace_snapshots()
        };

        Box::pin(async move {
            let mut out = Vec::new();
            for snapshot in &snapshots {
                out.extend(snapshot.workspace_symbols(&params.query, encoding));
            }

            Ok(Some(WorkspaceSymbolResponse::Flat(out)))
        })
    }

    fn semantic_tokens_full(
        &mut self,
        params: SemanticTokensParams,
    ) -> LspFuture<Option<SemanticTokensResult>> {
        snapshot_response(
            &self.state,
            &params.text_document.uri,
            move |path, encoding, _, snapshot| {
                snapshot
                    .semantic_tokens_full(&path, encoding)
                    .map(SemanticTokensResult::Tokens)
            },
        )
    }

    fn code_action(&mut self, params: CodeActionParams) -> LspFuture<Option<CodeActionResponse>> {
        snapshot_response(
            &self.state,
            &params.text_document.uri,
            move |path, encoding, config, snapshot| {
                snapshot.code_actions(
                    &path,
                    params.range,
                    encoding,
                    config.inferred_signature_hints_enabled,
                )
            },
        )
    }
}

fn snapshot_response<T>(
    state: &Arc<Mutex<ServerState>>,
    uri: &async_lsp::lsp_types::Url,
    f: impl FnOnce(PathBuf, PositionEncoding, ServerConfig, WorkspaceSnapshot) -> Option<T>
    + Send
    + 'static,
) -> LspFuture<Option<T>>
where
    T: Send + 'static,
{
    snapshot_result_response(state, uri, move |path, encoding, config, snapshot| {
        Ok(f(path, encoding, config, snapshot))
    })
}

fn snapshot_result_response<T>(
    state: &Arc<Mutex<ServerState>>,
    uri: &async_lsp::lsp_types::Url,
    f: impl FnOnce(PathBuf, PositionEncoding, ServerConfig, WorkspaceSnapshot) -> LspResult<Option<T>>
    + Send
    + 'static,
) -> LspFuture<Option<T>>
where
    T: Send + 'static,
{
    let snapshot = {
        let state = lock_state(state);
        state.snapshot_for_uri(uri)
    };

    Box::pin(async move {
        let Some((path, encoding, config, snapshot)) = snapshot else {
            return Ok(None);
        };

        f(path, encoding, config, snapshot)
    })
}

fn source_edit_response_error(error: SourceEditError) -> ResponseError {
    ResponseError::new(ErrorCode::REQUEST_FAILED, error)
}

fn ready_response<T>(value: T) -> LspFuture<T>
where
    T: Send + 'static,
{
    Box::pin(std::future::ready(Ok(value)))
}

fn publish_diagnostics(
    client: &ClientSocket,
    publications: Vec<(
        async_lsp::lsp_types::Url,
        Vec<async_lsp::lsp_types::Diagnostic>,
    )>,
) {
    for (uri, diagnostics) in publications {
        if let Err(err) = client.notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri,
            diagnostics,
            version: None,
        }) {
            tracing::debug!(error = %err, "Failed to publish Solveig diagnostics");
            return;
        }
    }
}

fn lock_state(state: &Mutex<ServerState>) -> MutexGuard<'_, ServerState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
        document_formatting_provider: Some(OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
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
                full: Some(SemanticTokensFullOptions::Bool(true)),
            },
        )),
        ..ServerCapabilities::default()
    }
}

fn formatting_width(options: &FormattingOptions) -> usize {
    match options.properties.get("lineWidth") {
        Some(FormattingProperty::Number(width)) if *width > 0 => {
            usize::try_from(*width).unwrap_or(DEFAULT_FORMAT_WIDTH)
        }
        _ => DEFAULT_FORMAT_WIDTH,
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

#[expect(
    deprecated,
    reason = "LSP clients may still send rootUri when workspaceFolders is absent"
)]
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
