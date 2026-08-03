//! Definition, reference, and symbol lookup over retained compiler artifacts.
//! These queries keep semantic indexing protocol-agnostic while LSP stays a DTO boundary.

use super::position::{
    DefTarget, TargetKind, target_at_offset_in_artifacts, target_at_position_in_artifacts,
};
use super::source_lookup::source_file_id;
use super::*;

pub(super) fn definition_in_artifacts(
    artifacts: &CompilationArtifacts,
    file: &Path,
    offset: usize,
) -> Option<FileSpan> {
    let def_id = def_target_at_offset(artifacts, file, offset)?.def_id;

    declaration_span(artifacts, def_id)
}

pub(super) fn definition_at_position_in_artifacts(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
) -> Option<FileSpan> {
    let def_id = def_target_at_position(artifacts, source_map, file, position, encoding)?.def_id;

    declaration_span(artifacts, def_id)
}

pub(super) fn references_in_artifacts(
    artifacts: &CompilationArtifacts,
    file: &Path,
    offset: usize,
    include_declaration: bool,
) -> Option<Vec<FileSpan>> {
    let def_id = def_target_at_offset(artifacts, file, offset)?.def_id;

    Some(references_to_def(artifacts, def_id, include_declaration))
}

pub(super) fn references_at_position_in_artifacts(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
    include_declaration: bool,
) -> Option<Vec<FileSpan>> {
    let def_id = def_target_at_position(artifacts, source_map, file, position, encoding)?.def_id;

    Some(references_to_def(artifacts, def_id, include_declaration))
}

pub(super) fn document_symbols_in_artifacts(
    artifacts: &CompilationArtifacts,
    file: &Path,
) -> Vec<SourceSymbol> {
    let Some(file_id) = source_file_id(artifacts, file) else {
        return Vec::new();
    };

    artifacts
        .source_symbols
        .iter()
        .filter(|symbol| symbol.file_span().file == file_id)
        .cloned()
        .collect()
}

pub(super) fn workspace_symbols_in_artifacts(
    artifacts: &CompilationArtifacts,
    query: &str,
) -> Vec<SourceSymbol> {
    let query = query.to_ascii_lowercase();
    let mut out = Vec::new();

    for symbol in &artifacts.source_symbols {
        push_matching_symbols(symbol, &query, &mut out);
    }

    out.sort_by_key(symbol_sort_key);
    out
}

pub(super) fn def_target_at_offset(
    artifacts: &CompilationArtifacts,
    file: &Path,
    offset: usize,
) -> Option<DefTarget> {
    def_target_from_kind(target_at_offset_in_artifacts(artifacts, file, offset)?.kind)
}

pub(super) fn def_target_at_position(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
) -> Option<DefTarget> {
    def_target_from_kind(
        target_at_position_in_artifacts(artifacts, source_map, file, position, encoding)?.kind,
    )
}

fn def_target_from_kind(kind: TargetKind) -> Option<DefTarget> {
    match kind {
        TargetKind::Definition(target) | TargetKind::Reference(target) => Some(target),
        TargetKind::Local
        | TargetKind::Module(_)
        | TargetKind::Literal(_)
        | TargetKind::Identifier
        | TargetKind::Keyword
        | TargetKind::Punctuation
        | TargetKind::Trivia => None,
    }
}

pub(super) fn declaration_span(
    artifacts: &CompilationArtifacts,
    def_id: DefId,
) -> Option<FileSpan> {
    artifacts
        .declaration_spans
        .iter()
        .filter(|(_, candidate)| *candidate == def_id)
        .map(|(span, _)| *span)
        .min_by_key(span_sort_key)
}

pub(super) fn references_to_def(
    artifacts: &CompilationArtifacts,
    def_id: DefId,
    include_declaration: bool,
) -> Vec<FileSpan> {
    let mut out = Vec::new();

    if include_declaration {
        out.extend(
            artifacts
                .declaration_spans
                .iter()
                .filter(|(_, candidate)| *candidate == def_id)
                .map(|(span, _)| *span),
        );
    }

    for resolutions in artifacts.resolutions.values() {
        for (span, candidate) in resolutions {
            if *candidate != def_id {
                continue;
            }

            if !include_declaration && is_declaration_span(artifacts, *span, def_id) {
                continue;
            }

            if span.is_empty() {
                continue;
            }

            out.push(*span);
        }
    }

    out.sort_by_key(span_sort_key);
    out.dedup();
    out
}

pub(super) fn is_declaration_span(
    artifacts: &CompilationArtifacts,
    span: FileSpan,
    def_id: DefId,
) -> bool {
    artifacts
        .declaration_spans
        .iter()
        .any(|(decl_span, candidate)| *candidate == def_id && *decl_span == span)
}

fn push_matching_symbols(symbol: &SourceSymbol, query: &str, out: &mut Vec<SourceSymbol>) {
    if query.is_empty() || symbol.name().to_ascii_lowercase().contains(query) {
        out.push(symbol.clone());
    }

    for child in symbol.children() {
        push_matching_symbols(child, query, out);
    }
}

fn symbol_sort_key(symbol: &SourceSymbol) -> (FileId, u32, u32, String) {
    let span = symbol.selection_span();

    (
        span.file,
        span.span.start,
        span.span.end,
        symbol.name().to_string(),
    )
}

fn span_sort_key(span: &FileSpan) -> (FileId, u32, u32) {
    (span.file, span.span.start, span.span.end)
}
