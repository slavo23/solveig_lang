//! Conversion from driver code actions into LSP quick fixes.

use std::collections::HashMap;

use lsp_types::{CodeAction, CodeActionKind, CodeActionOrCommand, TextEdit, WorkspaceEdit};
use solveig_base::{PositionEncoding, SourceMap};
use solveig_driver::SourceCodeAction;

use crate::diagnostics::{lsp_range, uri_for_file};

pub(crate) fn lsp_code_actions(
    actions: &[SourceCodeAction],
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Vec<CodeActionOrCommand> {
    actions
        .iter()
        .filter_map(|action| lsp_code_action(action, source_map, encoding))
        .collect()
}

fn lsp_code_action(
    action: &SourceCodeAction,
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Option<CodeActionOrCommand> {
    let mut changes = HashMap::new();

    for edit in action.edits() {
        let uri = uri_for_file(source_map, edit.file())?;
        changes
            .entry(uri)
            .or_insert_with(Vec::new)
            .push(TextEdit::new(
                lsp_range(source_map, edit.file_span(), encoding),
                edit.new_text().to_string(),
            ));
    }

    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: action.title().to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: None,
        disabled: None,
        data: None,
    }))
}
