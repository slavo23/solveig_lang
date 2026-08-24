//! Conversion from driver editor-feature payloads into LSP protocol types.

use std::collections::HashMap;

use async_lsp::lsp_types::{
    DocumentHighlight, DocumentHighlightKind, DocumentLink, Documentation, FoldingRange,
    FoldingRangeKind, ParameterInformation, ParameterLabel, PrepareRenameResponse, SelectionRange,
    SignatureHelp, SignatureInformation, TextEdit, WorkspaceEdit,
};
use solveig_base::{FileSpan, PositionEncoding, SourceMap};
use solveig_ide::{
    SourceDocumentHighlight, SourceDocumentHighlightKind, SourceDocumentLink, SourceFoldingRange,
    SourceFoldingRangeKind, SourceSelectionRange, SourceSignatureHelp, SourceTextEdit,
};

use crate::diagnostics::{lsp_range, uri_for_file};

pub(crate) fn lsp_document_highlights(
    highlights: &[SourceDocumentHighlight],
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Vec<DocumentHighlight> {
    highlights
        .iter()
        .map(|highlight| DocumentHighlight {
            range: lsp_range(source_map, highlight.file_span(), encoding),
            kind: Some(document_highlight_kind(highlight.kind())),
        })
        .collect()
}

pub(crate) fn lsp_folding_ranges(
    ranges: &[SourceFoldingRange],
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Vec<FoldingRange> {
    ranges
        .iter()
        .filter_map(|range| lsp_folding_range(range, source_map, encoding))
        .collect()
}

pub(crate) fn lsp_selection_ranges(
    ranges: &[SourceSelectionRange],
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Vec<SelectionRange> {
    ranges
        .iter()
        .map(|range| lsp_selection_range(range, source_map, encoding))
        .collect()
}

pub(crate) fn lsp_signature_help(help: &SourceSignatureHelp) -> SignatureHelp {
    SignatureHelp {
        signatures: help
            .signatures()
            .iter()
            .map(|signature| SignatureInformation {
                label: signature.label().to_string(),
                documentation: signature
                    .documentation()
                    .map(|doc| Documentation::String(doc.to_string())),
                parameters: Some(
                    signature
                        .parameters()
                        .iter()
                        .map(|parameter| ParameterInformation {
                            label: ParameterLabel::Simple(parameter.clone()),
                            documentation: None,
                        })
                        .collect(),
                ),
                active_parameter: None,
            })
            .collect(),
        active_signature: Some(help.active_signature()),
        active_parameter: Some(help.active_parameter()),
    }
}

pub(crate) fn lsp_prepare_rename(
    span: FileSpan,
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Option<PrepareRenameResponse> {
    Some(PrepareRenameResponse::RangeWithPlaceholder {
        range: lsp_range(source_map, span, encoding),
        placeholder: source_text_for_span(source_map, span)?.to_string(),
    })
}

pub(crate) fn lsp_workspace_edit(
    edits: &[SourceTextEdit],
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Option<WorkspaceEdit> {
    let mut changes = HashMap::new();

    for edit in edits {
        let uri = uri_for_file(source_map, edit.file())?;
        changes
            .entry(uri)
            .or_insert_with(Vec::new)
            .push(TextEdit::new(
                lsp_range(source_map, edit.file_span(), encoding),
                edit.new_text().to_string(),
            ));
    }

    Some(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

pub(crate) fn lsp_text_edits(
    edits: &[SourceTextEdit],
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Vec<TextEdit> {
    edits
        .iter()
        .map(|edit| {
            TextEdit::new(
                lsp_range(source_map, edit.file_span(), encoding),
                edit.new_text().to_string(),
            )
        })
        .collect()
}

pub(crate) fn lsp_document_links(
    links: &[SourceDocumentLink],
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Vec<DocumentLink> {
    links
        .iter()
        .map(|link| DocumentLink {
            range: lsp_range(source_map, link.file_span(), encoding),
            target: async_lsp::lsp_types::Url::from_file_path(link.target()).ok(),
            tooltip: link.tooltip().map(str::to_string),
            data: None,
        })
        .collect()
}

fn document_highlight_kind(kind: SourceDocumentHighlightKind) -> DocumentHighlightKind {
    match kind {
        SourceDocumentHighlightKind::Read => DocumentHighlightKind::READ,
        SourceDocumentHighlightKind::Write => DocumentHighlightKind::WRITE,
    }
}

fn lsp_folding_range(
    range: &SourceFoldingRange,
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Option<FoldingRange> {
    let lsp = lsp_range(source_map, range.file_span(), encoding);
    let (end_line, end_character) = if lsp.end.character == 0 && lsp.end.line > lsp.start.line {
        (lsp.end.line.saturating_sub(1), None)
    } else {
        (lsp.end.line, Some(lsp.end.character))
    };

    if end_line <= lsp.start.line {
        return None;
    }

    Some(FoldingRange {
        start_line: lsp.start.line,
        start_character: Some(lsp.start.character),
        end_line,
        end_character,
        kind: Some(folding_range_kind(range.kind())),
        collapsed_text: None,
    })
}

fn folding_range_kind(kind: SourceFoldingRangeKind) -> FoldingRangeKind {
    match kind {
        SourceFoldingRangeKind::Region => FoldingRangeKind::Region,
        SourceFoldingRangeKind::Comment => FoldingRangeKind::Comment,
    }
}

fn lsp_selection_range(
    range: &SourceSelectionRange,
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> SelectionRange {
    SelectionRange {
        range: lsp_range(source_map, range.file_span(), encoding),
        parent: range
            .parent()
            .map(|parent| Box::new(lsp_selection_range(parent, source_map, encoding))),
    }
}

fn source_text_for_span(source_map: &SourceMap, span: FileSpan) -> Option<&str> {
    let source = source_map.files().get(span.file.0 as usize)?;

    source.src.get(span.span.to_usize_range())
}
