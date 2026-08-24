//! Package-aware formatter queries over retained CST position indexes.

use super::*;

pub(super) fn format_document_in_artifacts(
    artifacts: &CompilationArtifacts,
    project: &Project,
    source_map: &SourceMap,
    file: &Path,
    width: usize,
) -> Result<Vec<SourceTextEdit>, SourceEditError> {
    source_edit::ensure_source_editable(project, source_map, file)?;
    let Some((file, source, index)) = formatting_input(artifacts, source_map, file) else {
        return Ok(Vec::new());
    };
    if width == 0 || !index.is_format_safe() {
        return Ok(Vec::new());
    }

    let formatted = solveig_fmt::format(index.syntax_tree(), source, width);
    if formatted == source {
        return Ok(Vec::new());
    }

    Ok(vec![SourceTextEdit::new(
        file,
        Span::from_usize_range(0..source.len()),
        formatted,
    )])
}

pub(super) fn format_range_in_artifacts(
    artifacts: &CompilationArtifacts,
    project: &Project,
    source_map: &SourceMap,
    file: &Path,
    positions: (SourcePosition, SourcePosition),
    encoding: PositionEncoding,
    width: usize,
) -> Result<Vec<SourceTextEdit>, SourceEditError> {
    source_edit::ensure_source_editable(project, source_map, file)?;
    let Some((file, source, index)) = formatting_input(artifacts, source_map, file) else {
        return Ok(Vec::new());
    };
    if width == 0 || !index.is_format_safe() {
        return Ok(Vec::new());
    }

    let start = source_map.offset(file, positions.0, encoding);
    let end = source_map.offset(file, positions.1, encoding);
    let requested = Span::new(start.min(end), start.max(end));
    let Some(definition) = index.top_level_definition_intersecting(requested) else {
        return Ok(Vec::new());
    };
    let Some((range, formatted)) =
        solveig_fmt::format_subtree(index.syntax_tree(), source, definition, width)
    else {
        return Ok(Vec::new());
    };
    let Some(original) = source.get(range.clone()) else {
        return Ok(Vec::new());
    };
    if original == formatted {
        return Ok(Vec::new());
    }

    Ok(vec![SourceTextEdit::new(
        file,
        Span::from_usize_range(range),
        formatted,
    )])
}

fn formatting_input<'a>(
    artifacts: &'a CompilationArtifacts,
    source_map: &'a SourceMap,
    file: &Path,
) -> Option<(FileId, &'a str, &'a PositionIndex)> {
    let file = source_lookup::source_file_id_in_source_map(source_map, file)?;
    let source = source_map.files().get(file.0 as usize)?.src.as_str();
    let index = artifacts.position_indexes.get(&file)?;

    Some((file, source, index))
}
