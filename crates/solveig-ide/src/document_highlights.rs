//! Source-level document highlight collection.

use super::navigation::{def_target_at_position, is_declaration_span, references_to_def};
use super::*;

pub(super) fn document_highlights_at_position_in_artifacts(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
) -> Vec<SourceDocumentHighlight> {
    let Some(candidate) = def_target_at_position(artifacts, source_map, file, position, encoding)
    else {
        return Vec::new();
    };
    let file_id = candidate.span.file;

    references_to_def(artifacts, candidate.def_id, true)
        .into_iter()
        .filter(|span| span.file == file_id)
        .map(|span| {
            let kind = if is_declaration_span(artifacts, span, candidate.def_id) {
                SourceDocumentHighlightKind::Write
            } else {
                SourceDocumentHighlightKind::Read
            };
            SourceDocumentHighlight::new(span, kind)
        })
        .collect()
}
