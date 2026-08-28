//! Diagnostic rendering for Solveig compiler messages.
//! This crate adapts structured diagnostics onto terminal and string outputs.

use std::error::Error;
use std::io::{self, IsTerminal, Write};

use ariadne::{Config, Label as AriadneLabel, Report, ReportKind, sources};
use solveig_base::SourceMap;
use solveig_base::{Diagnostic, Severity};

type AriadneSpan = (String, std::ops::Range<usize>);

/// Emits diagnostics to standard error using terminal color detection.
pub fn emit_diagnostics(
    diagnostics: &[Diagnostic],
    source_map: &SourceMap,
) -> Result<(), Box<dyn Error>> {
    let stderr = io::stderr();
    let color = stderr.is_terminal();

    write_diagnostics(diagnostics, source_map, stderr, color)?;
    Ok(())
}

/// Renders diagnostics into a string without terminal color escapes.
pub fn render_diagnostics_no_color(
    diagnostics: &[Diagnostic],
    source_map: &SourceMap,
) -> Result<String, Box<dyn Error>> {
    let mut out = Vec::new();
    write_diagnostics(diagnostics, source_map, &mut out, false)?;

    Ok(String::from_utf8(out)?)
}

/// Writes diagnostics to the provided writer with explicit color control.
pub fn write_diagnostics<W: Write>(
    diagnostics: &[Diagnostic],
    source_map: &SourceMap,
    mut writer: W,
    color: bool,
) -> io::Result<()> {
    for diagnostic in diagnostics {
        write_diagnostic(diagnostic, source_map, &mut writer, color)?;
    }

    Ok(())
}

fn write_diagnostic<W: Write>(
    diagnostic: &Diagnostic,
    source_map: &SourceMap,
    writer: &mut W,
    color: bool,
) -> io::Result<()> {
    let Some(anchor) = diagnostic.anchor_span() else {
        writeln!(
            writer,
            "[{}] {}: {}",
            diagnostic.code,
            severity_name(diagnostic.severity),
            diagnostic.message
        )?;
        for note in &diagnostic.notes {
            writeln!(writer, "  note: {note}")?;
        }
        return Ok(());
    };

    let anchor_span = ariadne_span(source_map, *anchor);
    let mut builder = Report::build(report_kind(diagnostic.severity), anchor_span)
        .with_config(Config::default().with_color(color))
        .with_code(diagnostic.code)
        .with_message(diagnostic.message.clone());

    for label in &diagnostic.labels {
        builder = builder.with_label(
            AriadneLabel::new(ariadne_span(source_map, label.span))
                .with_message(label.message.clone()),
        );
    }

    for note in &diagnostic.notes {
        builder = builder.with_note(note.clone());
    }

    builder.finish().write(source_cache(source_map), writer)
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "Error",
        Severity::Warning => "Warning",
        Severity::Note => "Note",
    }
}

fn report_kind(severity: Severity) -> ReportKind<'static> {
    match severity {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Note => ReportKind::Advice,
    }
}

fn ariadne_span(source_map: &SourceMap, span: solveig_base::FileSpan) -> AriadneSpan {
    let file = source_map.lookup(span.file);
    (file.path.display().to_string(), span.span.to_usize_range())
}

fn source_cache(source_map: &SourceMap) -> impl ariadne::Cache<String> {
    sources(
        source_map
            .files()
            .iter()
            .map(|file| (file.path.display().to_string(), file.src.clone())),
    )
}

#[cfg(test)]
mod tests {
    use solveig_base::Label;
    use solveig_base::{FileSpan, SourceMap, Span};

    use super::*;

    #[test]
    fn renders_no_color_diagnostic_from_source_map() {
        let mut source_map = SourceMap::new();
        let file = source_map.add_file("main.sv", "x = y\n");
        let diagnostic = Diagnostic::error("T0001", "bad name").with_label(Label::primary(
            FileSpan::new(file, Span::new(4, 5)),
            "unknown here",
        ));

        let rendered = render_diagnostics_no_color(&[diagnostic], &source_map).unwrap();

        assert!(rendered.contains("Error: bad name"));
        assert!(rendered.contains("[T0001]"));
        assert!(rendered.contains("unknown here"));
    }

    #[test]
    fn renders_diagnostic_without_source_labels() {
        let diagnostic = Diagnostic::error("CORE0001", "core verification failed")
            .with_note("inferred type did not match");

        let rendered = render_diagnostics_no_color(&[diagnostic], &SourceMap::new()).unwrap();

        assert!(rendered.contains("[CORE0001] Error: core verification failed"));
        assert!(rendered.contains("note: inferred type did not match"));
    }
}
