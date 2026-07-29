use lsp_types::{
    ClientCapabilities, DidOpenTextDocumentParams, DocumentSymbolParams, DocumentSymbolResponse,
    GotoDefinitionParams, GotoDefinitionResponse, InitializeParams, PartialResultParams, Position,
    ReferenceContext, ReferenceParams, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Url, WorkDoneProgressParams, WorkspaceSymbolParams,
};
use tower_lsp::{LanguageServer, LspService};

use super::{temp_root, write_source};
use crate::server::LspServer;

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
    let _ = std::fs::remove_dir_all(project_root);
}
