//! Source-level prepare-rename and rename edits over semantic references.

use super::navigation::{declaration_span, def_target_at_position, references_to_def};
use super::position::DefTarget;
use super::*;

pub(super) fn prepare_rename_in_artifacts(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
) -> Option<FileSpan> {
    let candidate = rename_candidate_at_position(artifacts, source_map, file, position, encoding)?;
    if !renameable_def(artifacts, source_map, candidate.def_id) {
        return None;
    }
    let old_name = source_text(source_map, candidate.span)?;
    if !valid_existing_name(old_name) {
        return None;
    }

    Some(candidate.span)
}

pub(super) fn rename_edits_in_artifacts(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
    new_name: &str,
) -> Vec<SourceTextEdit> {
    let Some(candidate) =
        rename_candidate_at_position(artifacts, source_map, file, position, encoding)
    else {
        return Vec::new();
    };
    if !renameable_def(artifacts, source_map, candidate.def_id) {
        return Vec::new();
    }
    let Some(old_name) = source_text(source_map, candidate.span) else {
        return Vec::new();
    };
    if !valid_new_name_for_old_name(new_name, old_name) {
        return Vec::new();
    }

    let mut spans = references_to_def(artifacts, candidate.def_id, true)
        .into_iter()
        .filter(|span| editable_file(source_map, span.file))
        .collect::<Vec<_>>();
    spans.sort_by_key(|span| (span.file, span.span.start, span.span.end));
    spans.dedup();

    spans
        .into_iter()
        .map(|span| SourceTextEdit::new(span.file, span.span, new_name.to_string()))
        .collect()
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

fn renameable_def(artifacts: &CompilationArtifacts, source_map: &SourceMap, def_id: DefId) -> bool {
    declaration_span(artifacts, def_id).is_some_and(|span| editable_file(source_map, span.file))
}

fn editable_file(source_map: &SourceMap, file: FileId) -> bool {
    let path = &source_map.lookup(file).path;
    path.is_absolute() && !path.to_string_lossy().starts_with('<')
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
