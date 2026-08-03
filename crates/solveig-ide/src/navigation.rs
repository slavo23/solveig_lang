//! Definition, reference, and symbol lookup over retained compiler artifacts.
//! These queries keep semantic indexing protocol-agnostic while LSP stays a DTO boundary.

use super::source_lookup::{is_atomic_source_span, source_file_id, span_contains_offset, span_len};
use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct DefCandidate {
    pub(super) span: FileSpan,
    pub(super) def_id: DefId,
    priority: u8,
}

pub(super) fn definition_in_artifacts(
    artifacts: &CompilationArtifacts,
    file: &Path,
    offset: usize,
) -> Option<FileSpan> {
    let file_id = source_file_id(artifacts, file)?;
    let def_id = def_id_at_offset(artifacts, file_id, offset)?.def_id;

    declaration_span(artifacts, def_id)
}

pub(super) fn definition_at_position_in_artifacts(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
) -> Option<FileSpan> {
    let file_id = source_file_id(artifacts, file)?;
    let offset = source_map.offset(file_id, position, encoding) as usize;

    definition_in_artifacts_by_file_id(artifacts, file_id, offset)
}

pub(super) fn references_in_artifacts(
    artifacts: &CompilationArtifacts,
    file: &Path,
    offset: usize,
    include_declaration: bool,
) -> Option<Vec<FileSpan>> {
    let file_id = source_file_id(artifacts, file)?;
    let def_id = def_id_at_offset(artifacts, file_id, offset)?.def_id;

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
    let file_id = source_file_id(artifacts, file)?;
    let offset = source_map.offset(file_id, position, encoding) as usize;
    let def_id = def_id_at_offset(artifacts, file_id, offset)?.def_id;

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

fn definition_in_artifacts_by_file_id(
    artifacts: &CompilationArtifacts,
    file_id: FileId,
    offset: usize,
) -> Option<FileSpan> {
    let def_id = def_id_at_offset(artifacts, file_id, offset)?.def_id;

    declaration_span(artifacts, def_id)
}

pub(super) fn def_id_at_offset(
    artifacts: &CompilationArtifacts,
    file_id: FileId,
    offset: usize,
) -> Option<DefCandidate> {
    let mut best = None;

    for (span, def_id) in &artifacts.declaration_spans {
        if span.file == file_id && span_contains_offset(&span.span.to_usize_range(), offset) {
            consider_def_candidate(&mut best, *span, *def_id, 2);
        }
    }

    let source_file = artifacts.source_files.get(file_id.0 as usize);
    for resolutions in artifacts.resolutions.values() {
        for (span, def_id) in resolutions {
            if span.file == file_id
                && source_file
                    .is_some_and(|source_file| is_atomic_source_span(source_file, span.span))
                && span_contains_offset(&span.span.to_usize_range(), offset)
            {
                consider_def_candidate(&mut best, *span, *def_id, 1);
            }
        }
    }

    best
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

            if !is_user_reference_span(artifacts, *span) {
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

fn is_user_reference_span(artifacts: &CompilationArtifacts, span: FileSpan) -> bool {
    artifacts
        .source_files
        .get(span.file.0 as usize)
        .is_some_and(|source_file| is_atomic_source_span(source_file, span.span))
}

fn push_matching_symbols(symbol: &SourceSymbol, query: &str, out: &mut Vec<SourceSymbol>) {
    if query.is_empty() || symbol.name().to_ascii_lowercase().contains(query) {
        out.push(symbol.clone());
    }

    for child in symbol.children() {
        push_matching_symbols(child, query, out);
    }
}

fn consider_def_candidate(
    best: &mut Option<DefCandidate>,
    span: FileSpan,
    def_id: DefId,
    priority: u8,
) {
    let replace = best.as_ref().is_none_or(|current| {
        let current_len = span_len(&current.span.span.to_usize_range());
        let candidate_len = span_len(&span.span.to_usize_range());

        candidate_len < current_len
            || (candidate_len == current_len && priority >= current.priority)
    });

    if replace {
        *best = Some(DefCandidate {
            span,
            def_id,
            priority,
        });
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
