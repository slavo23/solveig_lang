use super::*;
use crate::server_config::{formatting_width, server_config_from_initialize};
use crate::workspace::DEFAULT_FORMAT_LINE_WIDTH;
use async_lsp::lsp_types::{
    CodeActionContext, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CompletionItemKind, CompletionParams, CompletionResponse,
    DidSaveTextDocumentParams, FoldingRangeProviderCapability, FormattingOptions,
    FormattingProperty, GeneralClientCapabilities, HoverContents, InlayHintKind, InlayHintLabel,
    InlayHintParams, InlayHintServerCapabilities, PartialResultParams, Position, Range,
    SelectionRangeProviderCapability, SemanticTokenType, SemanticTokensFullOptions,
    SemanticTokensParams, SemanticTokensResult, SemanticTokensServerCapabilities,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Url, VersionedTextDocumentIdentifier, WorkDoneProgressParams,
    WorkspaceFolder,
};
use async_lsp::{ClientSocket, LanguageServer};
use std::fs;
use std::ops::Deref;
use std::path::Path;
use tempfile::{Builder, TempDir};

mod editor_protocol;
mod navigation;

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

fn initialize_params(root_uri: Url) -> InitializeParams {
    InitializeParams {
        workspace_folders: Some(vec![WorkspaceFolder {
            uri: root_uri,
            name: "test".to_string(),
        }]),
        capabilities: ClientCapabilities::default(),
        ..InitializeParams::default()
    }
}

fn initialize_params_with_options(
    root_uri: Url,
    initialization_options: serde_json::Value,
) -> InitializeParams {
    InitializeParams {
        initialization_options: Some(initialization_options),
        ..initialize_params(root_uri)
    }
}

fn flush_checks(server: &LspServer) {
    let mut state = lock_state(&server.state);
    if let Some(handle) = state.pending_check.take() {
        handle.abort();
    }
    let _publications = state.check_all();
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
    assert_eq!(
        capabilities.document_formatting_provider,
        Some(OneOf::Left(true))
    );
    assert_eq!(
        capabilities.document_range_formatting_provider,
        Some(OneOf::Left(true))
    );
    let completion = capabilities
        .completion_provider
        .expect("completion provider should be advertised");
    assert_eq!(completion.resolve_provider, Some(false));
    assert_eq!(
        completion.trigger_characters,
        Some(vec![".".to_string(), "(".to_string(), ",".to_string()])
    );
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
        Some(SemanticTokensFullOptions::Bool(true))
    ));
}

#[test]
fn source_edit_refusals_are_clear_lsp_request_failures() {
    let error = source_edit_response_error(SourceEditError::DependencyPackage {
        path: std::path::PathBuf::from("/dependencies/toolkit/src/core.sv"),
    });

    assert_eq!(error.code, ErrorCode::REQUEST_FAILED);
    assert!(error.message.contains("dependency source"), "{error}");
    assert!(
        error
            .message
            .contains("open that package as the local project"),
        "{error}"
    );
}

#[test]
fn formatter_line_width_uses_one_custom_option_with_a_safe_default() {
    let mut options = FormattingOptions::default();
    assert_eq!(
        formatting_width(&options, DEFAULT_FORMAT_LINE_WIDTH),
        DEFAULT_FORMAT_LINE_WIDTH
    );
    assert_eq!(formatting_width(&options, 88), 88);

    options
        .properties
        .insert("lineWidth".to_string(), FormattingProperty::Number(72));
    assert_eq!(formatting_width(&options, DEFAULT_FORMAT_LINE_WIDTH), 72);

    options
        .properties
        .insert("lineWidth".to_string(), FormattingProperty::Number(0));
    assert_eq!(
        formatting_width(&options, DEFAULT_FORMAT_LINE_WIDTH),
        DEFAULT_FORMAT_LINE_WIDTH
    );
}

#[test]
fn initialize_options_configure_editor_hints() {
    let default_config = server_config_from_initialize(&InitializeParams::default());
    assert!(default_config.inlay_hints_enabled);
    assert!(!default_config.inferred_signature_hints_enabled);
    assert_eq!(default_config.format_line_width, DEFAULT_FORMAT_LINE_WIDTH);

    let configured = server_config_from_initialize(&InitializeParams {
        initialization_options: Some(serde_json::json!({
            "inlayHints": {
                "enabled": false
            },
            "diagnostics": {
                "inferredSignatures": {
                    "enabled": true
                }
            },
            "formatting": {
                "lineWidth": 72
            }
        })),
        ..InitializeParams::default()
    });

    assert!(!configured.inlay_hints_enabled);
    assert!(configured.inferred_signature_hints_enabled);
    assert_eq!(configured.format_line_width, 72);
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
    let mut server = LspServer::new(ClientSocket::new_closed());

    server
        .initialize(initialize_params(root_uri))
        .await
        .expect("initialize should succeed");

    assert!(
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "solveig".to_string(),
                    version: 1,
                    text: text.to_string(),
                },
            })
            .is_continue()
    );
    flush_checks(&server);

    let module_completion = server
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
    let CompletionResponse::Array(module_items) = module_completion else {
        panic!("completion should return an item array");
    };
    assert!(
        module_items
            .iter()
            .any(|item| item.label == "builtins" && item.kind == Some(CompletionItemKind::MODULE))
    );

    let export_completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(1, 24),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .expect("completion request should succeed")
        .expect("completion response should exist");
    let CompletionResponse::Array(export_items) = export_completion else {
        panic!("completion should return an item array");
    };

    let true_item = export_items
        .iter()
        .find(|item| item.label == "True")
        .unwrap_or_else(|| panic!("missing True completion: {export_items:#?}"));
    assert_eq!(true_item.kind, Some(CompletionItemKind::CONSTRUCTOR));
    assert!(
        true_item
            .detail
            .as_ref()
            .is_some_and(|detail| detail.contains("Bool"))
    );

    server.shutdown(()).await.expect("shutdown should succeed");
    drop(project_root);
}

#[tokio::test(flavor = "current_thread")]
async fn text_document_completion_returns_only_selected_module_exports() {
    let project_root = temp_root("server_header_completion");
    let source = project_root.join("scratch.sv");
    let text = r#"import std.channel()

main : Int
main = 1
"#;
    write_source(&source, text);
    let uri = Url::from_file_path(&source).expect("test path should become a file URI");
    let root_uri =
        Url::from_directory_path(&project_root).expect("test root should become a file URI");
    let mut server = LspServer::new(ClientSocket::new_closed());

    server
        .initialize(initialize_params(root_uri))
        .await
        .expect("initialize should succeed");
    assert!(
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "solveig".to_string(),
                    version: 1,
                    text: text.to_string(),
                },
            })
            .is_continue()
    );
    flush_checks(&server);

    let completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(0, 19),
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

    assert!(items.iter().any(|item| item.label == "send"));
    assert!(items.iter().any(|item| item.label == "newChan"));
    assert!(items.iter().all(|item| item.label != "map"));
    assert!(items.iter().all(|item| item.label != "readFile"));

    server.shutdown(()).await.expect("shutdown should succeed");
    drop(project_root);
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
    let mut server = LspServer::new(ClientSocket::new_closed());

    server
        .initialize(initialize_params(root_uri))
        .await
        .expect("initialize should succeed");

    assert!(
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "solveig".to_string(),
                    version: 1,
                    text: text.to_string(),
                },
            })
            .is_continue()
    );
    flush_checks(&server);

    let hints = server
        .inlay_hint(InlayHintParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: Range::new(Position::new(0, 0), Position::new(3, 0)),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("inlay-hint request should succeed")
        .expect("inlay-hint response should exist");

    let local_hint = hints
        .iter()
        .find(|hint| {
            matches!(&hint.label, InlayHintLabel::String(label) if label == ": Int")
                && hint.kind == Some(InlayHintKind::TYPE)
        })
        .unwrap_or_else(|| panic!("missing local type hint: {hints:#?}"));
    assert_eq!(local_hint.padding_left, Some(false));
    assert_eq!(local_hint.padding_right, Some(true));

    server.shutdown(()).await.expect("shutdown should succeed");
    drop(project_root);
}

#[tokio::test(flavor = "current_thread")]
async fn text_document_inlay_hint_respects_disabled_initialization_option() {
    let project_root = temp_root("server_inlay_hint_disabled");
    let source = project_root.join("scratch.sv");
    let text = r#"main =
    let local = 41 in
    local
"#;
    write_source(&source, text);
    let uri = Url::from_file_path(&source).expect("test path should become a file URI");
    let root_uri =
        Url::from_directory_path(&project_root).expect("test root should become a file URI");
    let mut server = LspServer::new(ClientSocket::new_closed());

    server
        .initialize(initialize_params_with_options(
            root_uri,
            serde_json::json!({
                "inlayHints": {
                    "enabled": false
                }
            }),
        ))
        .await
        .expect("initialize should succeed");

    assert!(
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "solveig".to_string(),
                    version: 1,
                    text: text.to_string(),
                },
            })
            .is_continue()
    );
    flush_checks(&server);

    let hints = server
        .inlay_hint(InlayHintParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: Range::new(Position::new(0, 0), Position::new(3, 0)),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("inlay-hint request should succeed")
        .expect("disabled inlay-hint response should still exist");

    assert!(hints.is_empty(), "disabled inlay hints leaked: {hints:#?}");

    server.shutdown(()).await.expect("shutdown should succeed");
    drop(project_root);
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
    let mut server = LspServer::new(ClientSocket::new_closed());

    server
        .initialize(initialize_params(root_uri))
        .await
        .expect("initialize should succeed");

    assert!(
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "solveig".to_string(),
                    version: 1,
                    text: text.to_string(),
                },
            })
            .is_continue()
    );
    flush_checks(&server);

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

    server.shutdown(()).await.expect("shutdown should succeed");
    drop(project_root);
}

#[test]
#[expect(
    deprecated,
    reason = "This test covers rootUri fallback when workspaceFolders is absent"
)]
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
    let mut server = LspServer::new(ClientSocket::new_closed());

    server
        .initialize(initialize_params(root_uri))
        .await
        .expect("initialize should succeed");

    assert!(
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
            .is_continue()
    );

    {
        let state = lock_state(&server.state);
        assert!(state.pending_check.is_some());
    }

    tokio::task::yield_now().await;
    tokio::time::advance(CHECK_DEBOUNCE).await;
    tokio::task::yield_now().await;

    {
        let state = lock_state(&server.state);
        assert!(state.pending_check.is_none());
        assert_eq!(state.previous_diagnostic_count(), 1);
    }

    assert!(
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
            .is_continue()
    );

    tokio::task::yield_now().await;
    tokio::time::advance(CHECK_DEBOUNCE).await;
    tokio::task::yield_now().await;

    {
        let state = lock_state(&server.state);
        assert!(state.pending_check.is_none());
        assert_eq!(state.previous_diagnostic_count(), 0);
    }

    server.shutdown(()).await.expect("shutdown should succeed");
    drop(project_root);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn save_after_adding_unannotated_function_keeps_hover_results() {
    let project_root = temp_root("server_save_hover");
    let source = project_root.join("scratch.sv");
    let initial = r#"main : Int
main = 41
"#;
    let edited = r#"helper x = x + 1

main : Int
main = helper 41
"#;
    write_source(&source, initial);
    let uri = Url::from_file_path(&source).expect("test path should become a file URI");
    let root_uri =
        Url::from_directory_path(&project_root).expect("test root should become a file URI");
    let mut server = LspServer::new(ClientSocket::new_closed());

    server
        .initialize(initialize_params_with_options(
            root_uri,
            serde_json::json!({
                "diagnostics": {
                    "inferredSignatures": {
                        "enabled": true
                    }
                }
            }),
        ))
        .await
        .expect("initialize should succeed");

    assert!(
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "solveig".to_string(),
                    version: 1,
                    text: initial.to_string(),
                },
            })
            .is_continue()
    );
    flush_checks(&server);

    let initial_main = server
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(0, 0),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("initial hover request should succeed")
        .expect("initial hover should exist");
    assert!(
        hover_markup_value(initial_main).contains("main : Int"),
        "initial main hover should work"
    );

    assert!(
        server
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: edited.to_string(),
                }],
            })
            .is_continue()
    );
    assert!(
        server
            .did_save(DidSaveTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                text: None,
            })
            .is_continue()
    );
    flush_checks(&server);

    tokio::task::yield_now().await;
    tokio::time::advance(CHECK_DEBOUNCE).await;
    tokio::task::yield_now().await;

    let helper_definition = server
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(0, 0),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("helper definition hover request should succeed")
        .expect("helper definition hover should exist");
    assert!(
        hover_markup_value(helper_definition).contains("helper : Int -> Int"),
        "helper definition hover should use edited text"
    );

    let helper_reference = server
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(3, 7),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("helper reference hover request should succeed")
        .expect("helper reference hover should exist");
    assert!(
        hover_markup_value(helper_reference).contains("helper : Int -> Int"),
        "helper reference hover should use edited text"
    );

    let retained_main = server
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(2, 0),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("main hover request should succeed after save")
        .expect("main hover should still exist after save");
    assert!(
        hover_markup_value(retained_main).contains("main : Int"),
        "existing hovers should survive an edit/save"
    );

    server.shutdown(()).await.expect("shutdown should succeed");
    drop(project_root);
}

#[tokio::test(flavor = "current_thread")]
async fn text_document_semantic_tokens_return_full_without_delta_state() {
    let project_root = temp_root("server_semantic_tokens");
    let source = project_root.join("scratch.sv");
    let initial = r#"main : Int
main = 41 + 1
"#;
    write_source(&source, initial);
    let uri = Url::from_file_path(&source).expect("test path should become a file URI");
    let root_uri =
        Url::from_directory_path(&project_root).expect("test root should become a file URI");
    let mut server = LspServer::new(ClientSocket::new_closed());

    server
        .initialize(initialize_params(root_uri))
        .await
        .expect("initialize should succeed");

    assert!(
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "solveig".to_string(),
                    version: 1,
                    text: initial.to_string(),
                },
            })
            .is_continue()
    );
    flush_checks(&server);

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
    assert!(full.result_id.is_none());
    assert!(!full.data.is_empty());
    assert_relative_tokens_well_formed(&full.data);

    server.shutdown(()).await.expect("shutdown should succeed");
    drop(project_root);
}

fn assert_relative_tokens_well_formed(tokens: &[async_lsp::lsp_types::SemanticToken]) {
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

fn hover_markup_value(hover: async_lsp::lsp_types::Hover) -> String {
    let HoverContents::Markup(markup) = hover.contents else {
        panic!("hover should use markdown markup content");
    };

    markup.value
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
    let mut server = LspServer::new(ClientSocket::new_closed());

    server
        .initialize(initialize_params(root_uri))
        .await
        .expect("initialize should succeed");

    assert!(
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
            .is_continue()
    );
    flush_checks(&server);

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
    assert!(!markup.value.contains("```solveig\n answer : Int"));
    assert!(markup.value.contains("The answer used by hover."));
    assert!(markup.value.contains("Defined in `scratch`"));
    assert_eq!(
        hover.range.expect("hover should carry a range").start.line,
        1
    );

    server.shutdown(()).await.expect("shutdown should succeed");
    drop(project_root);
}

#[tokio::test(flavor = "current_thread")]
async fn text_document_hover_renders_prelude_type_alias() {
    let project_root = temp_root("server_type_alias_hover");
    let source = project_root.join("scratch.sv");
    let text = "main : Task ()\nmain = pureIO ()\n";
    write_source(&source, text);
    let uri = Url::from_file_path(&source).expect("test path should become a file URI");
    let root_uri =
        Url::from_directory_path(&project_root).expect("test root should become a file URI");
    let mut server = LspServer::new(ClientSocket::new_closed());

    server
        .initialize(initialize_params(root_uri))
        .await
        .expect("initialize should succeed");
    assert!(
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "solveig".to_string(),
                    version: 1,
                    text: text.to_string(),
                },
            })
            .is_continue()
    );
    flush_checks(&server);

    let hover = server
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(0, 7),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("hover request should succeed")
        .expect("Task alias should have LSP hover");

    let range = hover.range.expect("Task hover should carry a range");
    let markdown = hover_markup_value(hover);
    assert!(markdown.contains("```solveig\ntype Task a = IO IOError a\n```"));
    assert!(
        markdown.contains("An IO action that can fail while interacting with the outside world.")
    );
    assert!(markdown.contains("Defined in `prelude.builtins`"));
    assert_eq!(range, Range::new(Position::new(0, 7), Position::new(0, 11)));

    server.shutdown(()).await.expect("shutdown should succeed");
    drop(project_root);
}
