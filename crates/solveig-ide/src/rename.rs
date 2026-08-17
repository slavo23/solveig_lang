//! Source-level prepare-rename and rename edits over semantic references.

use super::navigation::{declaration_span, def_target_at_position, references_to_def};
use super::position::DefTarget;
use super::*;

pub(super) fn prepare_rename_in_artifacts(
    artifacts: &CompilationArtifacts,
    project: &Project,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
) -> Result<Option<FileSpan>, SourceEditError> {
    let Some(candidate) =
        rename_candidate_at_position(artifacts, source_map, file, position, encoding)
    else {
        return Ok(None);
    };
    let Some(old_name) = source_text(source_map, candidate.span) else {
        return Ok(None);
    };
    if !valid_existing_name(old_name) {
        return Ok(None);
    }
    let Some(_) = editable_rename_spans(artifacts, project, source_map, candidate.def_id)? else {
        return Ok(None);
    };

    Ok(Some(candidate.span))
}

pub(super) fn rename_edits_in_artifacts(
    artifacts: &CompilationArtifacts,
    project: &Project,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
    new_name: &str,
) -> Result<Vec<SourceTextEdit>, SourceEditError> {
    let Some(candidate) =
        rename_candidate_at_position(artifacts, source_map, file, position, encoding)
    else {
        return Ok(Vec::new());
    };
    let Some(old_name) = source_text(source_map, candidate.span) else {
        return Ok(Vec::new());
    };
    if !valid_new_name_for_old_name(new_name, old_name) {
        return Ok(Vec::new());
    }

    let Some(spans) = editable_rename_spans(artifacts, project, source_map, candidate.def_id)?
    else {
        return Ok(Vec::new());
    };

    Ok(spans
        .into_iter()
        .map(|span| SourceTextEdit::new(span.file, span.span, new_name.to_string()))
        .collect())
}

fn rename_candidate_at_position(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
) -> Option<DefTarget> {
    def_target_at_position(artifacts, source_map, file, position, encoding)
}

fn editable_rename_spans(
    artifacts: &CompilationArtifacts,
    project: &Project,
    source_map: &SourceMap,
    def_id: DefId,
) -> Result<Option<Vec<FileSpan>>, SourceEditError> {
    if declaration_span(artifacts, def_id).is_none() {
        return Ok(None);
    }

    let mut spans = references_to_def(artifacts, def_id, true);
    spans.sort_by_key(|span| (span.file, span.span.start, span.span.end));
    spans.dedup();
    for span in &spans {
        source_edit::ensure_file_id_editable(project, source_map, span.file)?;
    }

    Ok(Some(spans))
}

fn source_text(source_map: &SourceMap, span: FileSpan) -> Option<&str> {
    source_map
        .lookup(span.file)
        .src
        .get(span.span.to_usize_range())
}

fn valid_existing_name(name: &str) -> bool {
    valid_identifier(name) || valid_operator(name)
}

fn valid_new_name_for_old_name(new_name: &str, old_name: &str) -> bool {
    if old_name.chars().all(is_operator_char) {
        return valid_operator(new_name);
    }
    if old_name
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
    {
        return valid_constructor_identifier(new_name);
    }

    valid_value_identifier(new_name)
}

fn valid_identifier(name: &str) -> bool {
    valid_value_identifier(name) || valid_constructor_identifier(name)
}

fn valid_value_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_lowercase() || first == '_') && chars.all(is_identifier_tail)
}

fn valid_constructor_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_uppercase() && chars.all(is_identifier_tail)
}

fn valid_operator(name: &str) -> bool {
    !name.is_empty() && name.chars().all(is_operator_char)
}

fn is_identifier_tail(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '\''
}

fn is_operator_char(ch: char) -> bool {
    matches!(
        ch,
        '!' | '#'
            | '$'
            | '%'
            | '&'
            | '*'
            | '+'
            | '.'
            | '/'
            | '<'
            | '='
            | '>'
            | '?'
            | '@'
            | '\\'
            | '^'
            | '|'
            | '-'
            | '~'
            | ':'
    )
}
