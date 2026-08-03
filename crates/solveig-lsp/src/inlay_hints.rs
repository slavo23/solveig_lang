//! Conversion from driver inlay hints into LSP inlay hint payloads.

use lsp_types::{InlayHint, InlayHintKind, InlayHintLabel};
use solveig_base::{PositionEncoding, SourceMap};
use solveig_ide::{SourceInlayHint, SourceInlayHintKind};

pub(crate) fn lsp_inlay_hints(
    hints: &[SourceInlayHint],
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Vec<InlayHint> {
    hints
        .iter()
        .map(|hint| lsp_inlay_hint(hint, source_map, encoding))
        .collect()
}

fn lsp_inlay_hint(
    hint: &SourceInlayHint,
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> InlayHint {
    let file_span = hint.file_span();
    let position = source_map.position(file_span.file, file_span.span.start, encoding);

    InlayHint {
        position: lsp_types::Position::new(position.line, position.character),
        label: InlayHintLabel::String(hint.label().to_string()),
        kind: Some(lsp_inlay_hint_kind(hint.kind())),
        text_edits: None,
        tooltip: None,
        padding_left: Some(false),
        padding_right: Some(true),
        data: None,
    }
}

fn lsp_inlay_hint_kind(kind: SourceInlayHintKind) -> InlayHintKind {
    match kind {
        SourceInlayHintKind::Type => InlayHintKind::TYPE,
    }
}
