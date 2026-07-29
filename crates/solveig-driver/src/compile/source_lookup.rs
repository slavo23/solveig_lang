//! Shared source-span lookup helpers for retained editor artifacts.

use super::*;

pub(super) fn source_file_id(artifacts: &CompilationArtifacts, file: &Path) -> Option<FileId> {
    let matching_file = |source: &&SourceFile| {
        source.path == file || (file.is_relative() && source.path.ends_with(file))
    };

    artifacts
        .source_files
        .iter()
        .enumerate()
        .rev()
        .find(|(index, source)| {
            matching_file(source) && file_id_has_editor_artifacts(artifacts, FileId(*index as u32))
        })
        .or_else(|| {
            artifacts
                .source_files
                .iter()
                .enumerate()
                .rev()
                .find(|(_, source)| matching_file(source))
        })
        .map(|(index, _)| FileId(index as u32))
}

fn file_id_has_editor_artifacts(artifacts: &CompilationArtifacts, file: FileId) -> bool {
    artifacts
        .module_files
        .values()
        .any(|module_file| *module_file == file)
        || artifacts
            .declaration_spans
            .iter()
            .any(|(span, _)| span.file == file)
        || artifacts
            .resolutions
            .values()
            .any(|resolutions| resolutions.iter().any(|(span, _)| span.file == file))
        || artifacts
            .local_hovers
            .iter()
            .any(|hover| hover.file_span().file == file)
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

pub(super) fn is_atomic_source_span(source_file: &SourceFile, span: Span) -> bool {
    let range = span.to_usize_range();
    let Some(text) = source_file.src.get(range) else {
        return false;
    };

    !text.is_empty() && !text.chars().any(char::is_whitespace)
}

pub(super) fn import_module_name_span(
    source: &str,
    import_span: Span,
    module_name: &str,
) -> Option<Span> {
    let text = source.get(import_span.to_usize_range())?;
    let import_start = text.find("import")?;
    let search_start = import_start + "import".len();
    let name_start = text[search_start..].find(module_name)? + search_start;
    let name_end = name_start + module_name.len();

    Some(Span::new(
        u32::try_from(import_span.start as usize + name_start).ok()?,
        u32::try_from(import_span.start as usize + name_end).ok()?,
    ))
}
