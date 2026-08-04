use async_lsp::lsp_types::{
    DidOpenTextDocumentParams, DocumentHighlightKind, DocumentHighlightParams, DocumentLinkParams,
    FoldingRangeKind, FoldingRangeParams, PartialResultParams, Position, PrepareRenameResponse,
    RenameParams, SelectionRangeParams, SignatureHelpParams, TextDocumentIdentifier,
    TextDocumentItem, TextDocumentPositionParams, Url, WorkDoneProgressParams,
};
use async_lsp::{ClientSocket, LanguageServer};

use super::{flush_checks, initialize_params, temp_root, write_source};
use crate::server::LspServer;

fn position_in_line(source: &str, line: &str, needle: &str) -> Position {
    let line_start = source
        .find(line)
        .unwrap_or_else(|| panic!("source should contain line `{line}`"));
    let needle_offset = line
        .find(needle)
        .unwrap_or_else(|| panic!("line `{line}` should contain `{needle}`"));
    let line_number = source
        .get(..line_start)
        .expect("test line start should be a UTF-8 boundary")
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count();

    Position::new(
        u32::try_from(line_number).expect("test line number should fit in u32"),
        u32::try_from(needle_offset).expect("test character offset should fit in u32"),
    )
}

#[tokio::test(flavor = "current_thread")]
async fn text_document_editor_protocol_requests_return_results() {
    let project_root = temp_root("server_editor_protocol");
    let main = project_root.join("src/main.sv");
    let helper = project_root.join("src/helper.sv");
    let main_text = r#"import helper(increment)

-- fold comment one
-- fold comment two
main : Int
main =
    increment 41

choose : Bool -> Int
choose flag =
    case flag of
        True => increment 1
        False => 0

maybeMain : Maybe Int
maybeMain = do
    let local = 41
    pure local
"#;
    write_source(
        &helper,
        r#"-- | Increments a number.
increment : Int -> Int
increment value = value + 1
"#,
    );
    write_source(&main, main_text);
    let main_uri = Url::from_file_path(&main).expect("test path should become a file URI");
    let helper_uri = Url::from_file_path(&helper).expect("test path should become a file URI");
    let root_uri =
        Url::from_directory_path(&project_root).expect("test root should become a file URI");
    let increment_position = position_in_line(main_text, "    increment 41", "increment");
    let argument_position = position_in_line(main_text, "    increment 41", "41");
    let mut server = LspServer::new(ClientSocket::new_closed());

    server
        .initialize(initialize_params(root_uri))
        .await
        .expect("initialize should succeed");

    assert!(
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: main_uri.clone(),
                    language_id: "solveig".to_string(),
                    version: 1,
                    text: main_text.to_string(),
                },
            })
            .is_continue()
    );
    flush_checks(&server);

    let highlights = server
        .document_highlight(DocumentHighlightParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: main_uri.clone(),
                },
                position: increment_position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("document-highlight request should succeed")
        .expect("document-highlight response should exist");
    assert!(highlights.len() >= 2, "{highlights:#?}");
    assert!(
        highlights
            .iter()
            .any(|highlight| highlight.kind == Some(DocumentHighlightKind::READ))
    );

    let folds = server
        .folding_range(FoldingRangeParams {
            text_document: TextDocumentIdentifier {
                uri: main_uri.clone(),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("folding-range request should succeed")
        .expect("folding-range response should exist");
    assert!(
        folds
            .iter()
            .any(|fold| fold.kind == Some(FoldingRangeKind::Comment))
    );
    assert!(
        folds
            .iter()
            .any(|fold| fold.kind == Some(FoldingRangeKind::Region))
    );

    let selection_ranges = server
        .selection_range(SelectionRangeParams {
            text_document: TextDocumentIdentifier {
                uri: main_uri.clone(),
            },
            positions: vec![increment_position],
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("selection-range request should succeed")
        .expect("selection-range response should exist");
    assert_eq!(selection_ranges.len(), 1);
    assert_eq!(selection_ranges[0].range.start, increment_position);
    assert_eq!(selection_ranges[0].range.end, Position::new(6, 13));
    assert!(selection_ranges[0].parent.is_some());

    let signature_help = server
        .signature_help(SignatureHelpParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: main_uri.clone(),
                },
                position: argument_position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            context: None,
        })
        .await
        .expect("signature-help request should succeed")
        .expect("signature-help response should exist");
    assert!(
        signature_help.signatures[0]
            .label
            .contains("increment : Int -> Int")
    );
    assert_eq!(signature_help.active_parameter, Some(0));

    let prepared = server
        .prepare_rename(TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: main_uri.clone(),
            },
            position: increment_position,
        })
        .await
        .expect("prepare-rename request should succeed")
        .expect("prepare-rename response should exist");
    let PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } = prepared else {
        panic!("prepare rename should return range with placeholder");
    };
    assert_eq!(range.start, increment_position);
    assert_eq!(placeholder, "increment");

    let edit = server
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: main_uri.clone(),
                },
                position: increment_position,
            },
            new_name: "bump".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("rename request should succeed")
        .expect("rename response should exist");
    let changes = edit
        .changes
        .expect("rename should return plain workspace changes");
    assert!(changes.contains_key(&main_uri));
    assert!(changes.contains_key(&helper_uri));
    assert!(
        changes
            .values()
            .flatten()
            .all(|text_edit| text_edit.new_text == "bump")
    );

    let links = server
        .document_link(DocumentLinkParams {
            text_document: TextDocumentIdentifier {
                uri: main_uri.clone(),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("document-link request should succeed")
        .expect("document-link response should exist");
    let helper_link = links
        .iter()
        .find(|link| link.target.as_ref() == Some(&helper_uri))
        .unwrap_or_else(|| panic!("missing helper document link: {links:#?}"));
    assert_eq!(helper_link.range.start, Position::new(0, 7));
    assert_eq!(helper_link.range.end, Position::new(0, 13));

    server.shutdown(()).await.expect("shutdown should succeed");
    drop(project_root);
}
