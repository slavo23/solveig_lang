//! Source-level document highlight collection.

use super::navigation::{def_id_at_offset, is_declaration_span, references_to_def};
use super::*;

pub(super) fn document_highlights_at_position_in_artifacts(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
) -> Vec<SourceDocumentHighlight> {
    let Some(file_id) = source_lookup::source_file_id_in_source_map(source_map, file) else {
        return Vec::new();
    };
    let offset = source_map.offset(file_id, position, encoding) as usize;
    let Some(candidate) = def_id_at_offset(artifacts, file_id, offset) else {
        return Vec::new();
    };

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
