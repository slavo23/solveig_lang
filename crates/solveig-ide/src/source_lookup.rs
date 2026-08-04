//! Shared source-span lookup helpers for retained editor artifacts.

use super::*;

fn file_id_from_index(index: usize) -> Option<FileId> {
    u32::try_from(index).ok().map(FileId)
}

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
            matching_file(source)
                && file_id_from_index(*index)
                    .is_some_and(|file| file_id_has_editor_artifacts(artifacts, file))
        })
        .or_else(|| {
            artifacts
                .source_files
                .iter()
                .enumerate()
                .rev()
                .find(|(_, source)| matching_file(source))
        })
        .and_then(|(index, _)| file_id_from_index(index))
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
        .and_then(|(index, _)| file_id_from_index(index))
}

pub(super) fn span_contains_offset(span: &Range<usize>, offset: usize) -> bool {
    if span.start == span.end {
        offset == span.start
    } else {
        span.start <= offset && offset < span.end
    }
}

pub(super) fn import_module_name_span(
    source: &str,
    import_span: Span,
    module_name: &str,
) -> Option<Span> {
    let text = source.get(import_span.to_usize_range())?;
    let import_start = text.find("import")?;
    let search_start = import_start + "import".len();
    let name_start = text.get(search_start..)?.find(module_name)? + search_start;
    let name_end = name_start + module_name.len();

    Some(Span::new(
        u32::try_from(import_span.start as usize + name_start).ok()?,
        u32::try_from(import_span.start as usize + name_end).ok()?,
    ))
}
