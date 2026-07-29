//! Conversion from Solveig diagnostics into LSP publish-diagnostics payloads.

use std::collections::BTreeMap;
use std::path::Path;

use lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, NumberOrString,
    Position, Range, Url,
};
use solveig_base::{
    Diagnostic as SolveigDiagnostic, FileSpan, PositionEncoding, Severity, SourceMap,
};

pub(crate) fn diagnostic_map(
    diagnostics: &[SolveigDiagnostic],
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> BTreeMap<Url, Vec<Diagnostic>> {
    let mut out = BTreeMap::<Url, Vec<Diagnostic>>::new();

    for diagnostic in diagnostics {
        let Some(anchor) = diagnostic.anchor_span().copied() else {
            continue;
        };
        let Some(uri) = uri_for_file(source_map, anchor.file) else {
            continue;
        };

        out.entry(uri)
            .or_default()
            .push(lsp_diagnostic(diagnostic, source_map, anchor, encoding));
    }

    out
}

pub(crate) fn missing_entry_diagnostic(path: &Path) -> Option<(Url, Vec<Diagnostic>)> {
    let uri = Url::from_file_path(path).ok()?;
    let range = Range::new(Position::new(0, 0), Position::new(0, 0));
    let diagnostic = Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String("DRV0001".to_string())),
        code_description: None,
        source: Some("solveig".to_string()),
        message: "could not find Solveig entry file".to_string(),
        related_information: None,
        tags: None,
        data: None,
    };

    Some((uri, vec![diagnostic]))
}

fn lsp_diagnostic(
    diagnostic: &SolveigDiagnostic,
    source_map: &SourceMap,
    anchor: FileSpan,
    encoding: PositionEncoding,
) -> Diagnostic {
    let related_information = related_information(diagnostic, source_map, anchor, encoding);
    let message = diagnostic_message(diagnostic);

    Diagnostic {
        range: lsp_range(source_map, anchor, encoding),
        severity: Some(lsp_severity(diagnostic.severity)),
        code: Some(NumberOrString::String(diagnostic.code.to_string())),
        code_description: None,
        source: Some("solveig".to_string()),
        message,
        related_information,
        tags: None,
        data: None,
    }
}

fn related_information(
    diagnostic: &SolveigDiagnostic,
    source_map: &SourceMap,
    anchor: FileSpan,
    encoding: PositionEncoding,
) -> Option<Vec<DiagnosticRelatedInformation>> {
    let mut related = Vec::new();

    for label in &diagnostic.labels {
        if label.primary && label.span == anchor {
            continue;
        }

        let Some(uri) = uri_for_file(source_map, label.span.file) else {
            continue;
        };

        related.push(DiagnosticRelatedInformation {
            location: Location::new(uri, lsp_range(source_map, label.span, encoding)),
            message: label.message.clone(),
        });
    }

    if related.is_empty() {
        None
    } else {
        Some(related)
    }
}

fn diagnostic_message(diagnostic: &SolveigDiagnostic) -> String {
    let mut message = diagnostic.message.clone();

    if let Some(primary) = diagnostic.labels.iter().find(|label| label.primary)
        && !primary.message.is_empty()
    {
        message.push_str(": ");
        message.push_str(&primary.message);
    }

    for note in &diagnostic.notes {
        message.push_str("\nNote: ");
        message.push_str(note);
    }

    message
}

pub(crate) fn lsp_range(
    source_map: &SourceMap,
    span: FileSpan,
    encoding: PositionEncoding,
) -> Range {
    let start = source_map.position(span.file, span.span.start, encoding);
    let end = source_map.position(span.file, span.span.end, encoding);

    Range::new(
        Position::new(start.line, start.character),
        Position::new(end.line, end.character),
    )
}

fn lsp_severity(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Note => DiagnosticSeverity::INFORMATION,
    }
}

fn uri_for_file(source_map: &SourceMap, file: solveig_base::FileId) -> Option<Url> {
    let source_file = source_map.files().get(file.0 as usize)?;
    Url::from_file_path(&source_file.path).ok()
}
