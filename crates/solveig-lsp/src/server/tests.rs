use super::*;
use lsp_types::{
    CodeActionContext, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CompletionItemKind, CompletionParams, CompletionResponse,
    FoldingRangeProviderCapability, GeneralClientCapabilities, HoverContents, InlayHintKind,
    InlayHintLabel, InlayHintParams, InlayHintServerCapabilities, PartialResultParams, Position,
    Range, SelectionRangeProviderCapability, SemanticTokenType, SemanticTokensDeltaParams,
    SemanticTokensFullDeltaResult, SemanticTokensFullOptions, SemanticTokensParams,
    SemanticTokensResult, SemanticTokensServerCapabilities, TextDocumentContentChangeEvent,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Url,
    VersionedTextDocumentIdentifier, WorkDoneProgressParams, WorkspaceFolder,
};
use std::fs;
use std::path::Path;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

mod editor_protocol;
mod navigation;

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
        capabilities.document_highlight_provider,
        Some(OneOf::Left(true))
    );
    let completion = capabilities
        .completion_provider
        .expect("completion provider should be advertised");
    assert_eq!(completion.resolve_provider, Some(false));
    assert_eq!(completion.trigger_characters, Some(vec![".".to_string()]));
    let signature_help = capabilities
        .signature_help_provider
        .expect("signature-help provider should be advertised");
    assert_eq!(
        signature_help.trigger_characters,
        Some(vec![" ".to_string(), "(".to_string()])
    );
    assert_eq!(
        signature_help.retrigger_characters,
        Some(vec![" ".to_string()])
    );
    let inlay_hints = capabilities
        .inlay_hint_provider
        .expect("inlay-hint provider should be advertised");
    let OneOf::Right(InlayHintServerCapabilities::Options(inlay_hint_options)) = inlay_hints else {
        panic!("inlay hints should use direct server options");
    };
    assert_eq!(inlay_hint_options.resolve_provider, Some(false));
    assert_eq!(
        capabilities.document_symbol_provider,
        Some(OneOf::Left(true))
    );
    assert_eq!(
        capabilities.workspace_symbol_provider,
        Some(OneOf::Left(true))
    );
    assert_eq!(
        capabilities.selection_range_provider,
        Some(SelectionRangeProviderCapability::Simple(true))
    );
    let rename = capabilities
        .rename_provider
        .expect("rename provider should be advertised");
    let OneOf::Right(rename_options) = rename else {
        panic!("rename should use direct server options");
    };
    assert_eq!(rename_options.prepare_provider, Some(true));
    let document_link = capabilities
        .document_link_provider
        .expect("document-link provider should be advertised");
    assert_eq!(document_link.resolve_provider, Some(false));
    assert_eq!(
        capabilities.folding_range_provider,
        Some(FoldingRangeProviderCapability::Simple(true))
    );
    let code_actions = capabilities
        .code_action_provider
        .expect("code-action provider should be advertised");
    let CodeActionProviderCapability::Options(code_action_options) = code_actions else {
        panic!("code actions should use direct server options");
    };
    assert_eq!(
        code_action_options.code_action_kinds,
        Some(vec![CodeActionKind::QUICKFIX])
    );
    let semantic_tokens = capabilities
        .semantic_tokens_provider
        .expect("semantic-token provider should be advertised");
    let SemanticTokensServerCapabilities::SemanticTokensOptions(options) = semantic_tokens else {
        panic!("semantic tokens should use direct server options");
    };
    assert!(
        options
            .legend
            .token_types
            .contains(&SemanticTokenType::NAMESPACE)
    );
    assert!(
        options
            .legend
            .token_types
            .contains(&SemanticTokenType::METHOD)
    );
    assert!(matches!(
        options.full,
        Some(SemanticTokensFullOptions::Delta { delta: Some(true) })
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn text_document_completion_returns_qualified_items() {
    let project_root = temp_root("server_completion");
    let source = project_root.join("scratch.sv");
    let text = r#"main : Bool
main = prelude.builtins.True
"#;
    write_source(&source, text);
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
                text: text.to_string(),
            },
        })
        .await;

    let completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(1, 15),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .expect("completion request should succeed")
        .expect("completion response should exist");
    let CompletionResponse::Array(items) = completion else {
        panic!("completion should return an item array");
    };

    let true_item = items
        .iter()
        .find(|item| item.label == "True")
        .unwrap_or_else(|| panic!("missing True completion: {items:#?}"));
    assert_eq!(true_item.kind, Some(CompletionItemKind::CONSTRUCTOR));
    assert!(
        true_item
            .detail
            .as_ref()
            .is_some_and(|detail| detail.contains("Bool"))
    );
    assert!(
        items
            .iter()
            .any(|item| item.label == "builtins" && item.kind == Some(CompletionItemKind::MODULE))
    );

    server.shutdown().await.expect("shutdown should succeed");
    let _ = fs::remove_dir_all(project_root);
}

#[tokio::test(flavor = "current_thread")]
async fn text_document_inlay_hint_returns_type_hints() {
    let project_root = temp_root("server_inlay_hint");
    let source = project_root.join("scratch.sv");
    let text = r#"main =
    let local = 41 in
    local
"#;
    write_source(&source, text);
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
                text: text.to_string(),
            },
        })
        .await;

    let hints = server
        .inlay_hint(InlayHintParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: Range::new(Position::new(0, 0), Position::new(3, 0)),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("inlay-hint request should succeed")
        .expect("inlay-hint response should exist");

    assert!(hints.iter().any(|hint| {
        matches!(&hint.label, InlayHintLabel::String(label) if label == ": Int")
            && hint.kind == Some(InlayHintKind::TYPE)
    }));

    server.shutdown().await.expect("shutdown should succeed");
    let _ = fs::remove_dir_all(project_root);
}

#[tokio::test(flavor = "current_thread")]
async fn text_document_code_action_returns_workspace_edit_for_hole() {
    let project_root = temp_root("server_code_action");
    let source = project_root.join("scratch.sv");
    let text = r#"main : Bool
main = ??
"#;
    write_source(&source, text);
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
                text: text.to_string(),
            },
        })
        .await;

    let actions = server
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: Range::new(Position::new(1, 7), Position::new(1, 9)),
            context: CodeActionContext::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("code-action request should succeed")
        .expect("code-action response should exist");

    let action = actions
        .iter()
        .find_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) if action.title == "Fill hole with `True`" => {
                Some(action)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing hole fill action: {actions:#?}"));
    let edit = action
        .edit
        .as_ref()
        .expect("code action should carry a workspace edit");
    let changes = edit
        .changes
        .as_ref()
        .expect("workspace edit should use plain changes");
    let text_edit = changes
        .get(&uri)
        .and_then(|edits| edits.first())
        .expect("workspace edit should update the open document");

    assert_eq!(
        text_edit.range,
        Range::new(Position::new(1, 7), Position::new(1, 9))
    );
    assert_eq!(text_edit.new_text, "True");

    server.shutdown().await.expect("shutdown should succeed");
    let _ = fs::remove_dir_all(project_root);
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
async fn text_document_semantic_tokens_return_full_and_delta() {
    let project_root = temp_root("server_semantic_tokens");
    let source = project_root.join("scratch.sv");
    let initial = r#"main : Int
main = 41 + 1
"#;
    write_source(&source, initial);
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
                text: initial.to_string(),
            },
        })
        .await;

    let full = server
        .semantic_tokens_full(SemanticTokensParams {
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        })
        .await
        .expect("semantic-token full request should succeed")
        .expect("semantic-token full response should exist");
    let SemanticTokensResult::Tokens(full) = full else {
        panic!("semantic-token full response should be a complete token set");
    };
    let previous_result_id = full
        .result_id
        .clone()
        .expect("semantic-token full response should carry a result id");

    assert!(!full.data.is_empty());
    assert_relative_tokens_well_formed(&full.data);

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
main = 42 + 1
"#
                .to_string(),
            }],
        })
        .await;

    let delta = server
        .semantic_tokens_full_delta(SemanticTokensDeltaParams {
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            previous_result_id,
        })
        .await
        .expect("semantic-token delta request should succeed")
        .expect("semantic-token delta response should exist");

    let SemanticTokensFullDeltaResult::TokensDelta(delta) = delta else {
        panic!("semantic-token delta response should be an edit");
    };

    assert!(delta.result_id.is_some());
    assert_eq!(delta.edits.len(), 1);
    assert_eq!(delta.edits[0].start, 0);
    assert_eq!(delta.edits[0].delete_count, (full.data.len() * 5) as u32);
    assert!(
        delta.edits[0]
            .data
            .as_ref()
            .is_some_and(|data| !data.is_empty())
    );

    server.shutdown().await.expect("shutdown should succeed");
    let _ = fs::remove_dir_all(project_root);
}

fn assert_relative_tokens_well_formed(tokens: &[lsp_types::SemanticToken]) {
    let mut line = 0u32;
    let mut character = 0u32;

    for (index, token) in tokens.iter().enumerate() {
        assert!(token.length > 0);
        if index == 0 {
            line = token.delta_line;
            character = token.delta_start;
            continue;
        }

        line = line.saturating_add(token.delta_line);
        if token.delta_line == 0 {
            assert!(token.delta_start > 0);
            character = character.saturating_add(token.delta_start);
        } else {
            character = token.delta_start;
        }
    }

    let _ = (line, character);
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
