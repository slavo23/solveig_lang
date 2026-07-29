//! Shared source-span lookup helpers for retained editor artifacts.

use super::*;

pub(super) fn source_file_id(artifacts: &CompilationArtifacts, file: &Path) -> Option<FileId> {
    artifacts
        .source_files
        .iter()
        .enumerate()
        .rev()
        .find(|(_, source)| source.path == file)
        .or_else(|| {
            artifacts
                .source_files
                .iter()
                .enumerate()
                .rev()
                .find(|(_, source)| file.is_relative() && source.path.ends_with(file))
        })
        .map(|(index, _)| FileId(index as u32))
}

pub(super) fn source_file_id_in_source_map(source_map: &SourceMap, file: &Path) -> Option<FileId> {
    source_map
        .files()
        .iter()
        .enumerate()
        .rev()
        .find(|(_, source)| source.path == file)
        .or_else(|| {
            source_map
                .files()
                .iter()
                .enumerate()
                .rev()
                .find(|(_, source)| file.is_relative() && source.path.ends_with(file))
        })
        .map(|(index, _)| FileId(index as u32))
}

pub(super) fn span_contains_offset(span: &Range<usize>, offset: usize) -> bool {
    if span.start == span.end {
        offset == span.start
    } else {
        span.start <= offset && offset < span.end
    }
}

pub(super) fn span_len(span: &Range<usize>) -> usize {
    span.end.saturating_sub(span.start)
}
