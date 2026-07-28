//! Structured diagnostic types shared by compiler stages.
//! Each diagnostic carries source labels that the reporting crate renders later.

use std::cell::RefCell;

use crate::source::FileSpan;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub span: FileSpan,
    pub message: String,
    pub primary: bool,
}

impl Label {
    pub fn primary(span: FileSpan, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            primary: true,
        }
    }

    pub fn secondary(span: FileSpan, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            primary: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn new(code: &'static str, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            code,
            severity,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn error(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(code, Severity::Error, message)
    }

    pub fn with_label(mut self, label: Label) -> Self {
        self.labels.push(label);
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_notes<I, S>(mut self, notes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.notes.extend(notes.into_iter().map(Into::into));
        self
    }

    pub fn anchor_span(&self) -> Option<&FileSpan> {
        self.labels
            .iter()
            .find(|label| label.primary)
            .map(|label| &label.span)
            .or_else(|| self.labels.first().map(|label| &label.span))
    }
}

pub trait IntoDiagnostic<Context = ()> {
    fn into_diagnostic(self, context: Context) -> Diagnostic;
}

#[derive(Debug, Default)]
pub struct DiagnosticSink {
    diagnostics: RefCell<Vec<Diagnostic>>,
}

impl DiagnosticSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, diagnostic: Diagnostic) {
        self.diagnostics.borrow_mut().push(diagnostic);
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .borrow()
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }

    pub fn drain(&self) -> Vec<Diagnostic> {
        self.diagnostics.borrow_mut().drain(..).collect()
    }

    pub fn sorted_by_file_and_span(&self) -> Vec<Diagnostic> {
        let mut diagnostics = self.diagnostics.borrow().clone();

        diagnostics.sort_by(|left, right| {
            let left_anchor = left.anchor_span();
            let right_anchor = right.anchor_span();

            match (left_anchor, right_anchor) {
                (Some(left), Some(right)) => left
                    .file
                    .cmp(&right.file)
                    .then(left.span.start.cmp(&right.span.start))
                    .then(left.span.end.cmp(&right.span.end)),
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, None) => left
                    .code
                    .cmp(right.code)
                    .then(left.message.cmp(&right.message)),
            }
        });

        diagnostics
    }

    pub fn len(&self) -> usize {
        self.diagnostics.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.diagnostics.borrow().is_empty()
    }
}

#[derive(Clone, Debug)]
pub enum TypeError {
    UnboundVariable {
        span: FileSpan,
        name: String,
    },
    UnknownConstructor {
        span: FileSpan,
        name: String,
    },
    UnknownPatternConstructor {
        span: FileSpan,
        name: String,
    },
    EmptyCaseExpression {
        span: FileSpan,
    },
    SubtypeMismatch {
        span: FileSpan,
        left: String,
        right: String,
    },
    InstantiateLeftFailed {
        span: FileSpan,
        evar: String,
        target: String,
    },
    InstantiateRightFailed {
        span: FileSpan,
        source: String,
        evar: String,
    },
    ConstructorNotApplicable {
        span: FileSpan,
        actual: String,
    },
    NonFunctionApplication {
        span: FileSpan,
        arg_span: FileSpan,
        actual: String,
    },
    NonPolymorphicTypeApplication {
        span: FileSpan,
        actual: String,
        ty_arg: String,
    },
    IllFormedAnnotation {
        span: FileSpan,
        ty: String,
        target: String,
    },
    MissingCaseResultExistential {
        span: FileSpan,
        evar: String,
        arm_index: usize,
    },
    UnsatisfiedConstraints {
        span: FileSpan,
        predicates: Vec<String>,
    },
    OverlappingInstances {
        span: FileSpan,
        class_name: String,
        instance_type: String,
    },
    OrphanInstance {
        span: FileSpan,
        class_name: String,
        instance_type: String,
    },
    UnreachablePattern {
        span: FileSpan,
    },
    MissingInstanceMethod {
        span: FileSpan,
        method: String,
        class_name: String,
    },
    UnsupportedFloatPattern {
        span: FileSpan,
    },

    /// Contextual wrapper around the real error that happened while checking an
    /// instance method body against the specialized class method type.
    ///
    /// Do not pre-render the inner error into a string here. Keeping the cause
    /// structured lets diagnostics underline the precise expression that failed
    /// and add instance/class context as notes.
    InstanceMethodTypeMismatch {
        span: FileSpan,
        method: String,
        class_name: String,
        expected: String,
        cause: Box<TypeError>,
    },
    NonExhaustivePatterns {
        span: FileSpan,
        scrutinee_type: String,
        missing: Vec<String>,
    },
}

#[derive(Clone, Debug)]
struct DiagnosticParts {
    code: &'static str,
    severity: Severity,
    message: String,
    labels: Vec<Label>,
    notes: Vec<String>,
}

impl DiagnosticParts {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self::with_severity(code, Severity::Error, message)
    }

    fn warning(code: &'static str, message: impl Into<String>) -> Self {
        Self::with_severity(code, Severity::Warning, message)
    }

    fn with_severity(code: &'static str, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            code,
            severity,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    fn with_label(mut self, label: Label) -> Self {
        self.labels.push(label);
        self
    }

    fn with_labels(mut self, labels: Vec<Label>) -> Self {
        self.labels.extend(labels);
        self
    }

    fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    fn with_notes(mut self, notes: Vec<String>) -> Self {
        self.notes.extend(notes);
        self
    }

    fn anchor_span(&self) -> Option<FileSpan> {
        self.labels
            .iter()
            .find(|label| label.primary)
            .map(|label| label.span)
            .or_else(|| self.labels.first().map(|label| label.span))
    }

    fn into_diagnostic(self) -> Diagnostic {
        Diagnostic {
            code: self.code,
            severity: self.severity,
            message: self.message,
            labels: self.labels,
            notes: self.notes,
        }
    }
}

fn format_missing_patterns_note(scrutinee_type: &str, missing: &[String]) -> String {
    match missing {
        [] => format!("scrutinee type: `{scrutinee_type}`\nmissing pattern:\n  _"),
        [one] => format!("scrutinee type: `{scrutinee_type}`\nmissing pattern:\n  {one}"),
        many => format!(
            "scrutinee type: `{}`\nmissing pattern examples:\n  {}",
            scrutinee_type,
            many.join("\n  ")
        ),
    }
}

impl TypeError {
    pub fn to_diagnostic(&self) -> Diagnostic {
        self.diagnostic_parts().into_diagnostic()
    }

    fn diagnostic_parts(&self) -> DiagnosticParts {
        match self {
            TypeError::UnboundVariable { span, name } => DiagnosticParts::new(
                "TC0001",
                format!("variable `{name}` is not in scope"),
            )
            .with_label(Label::primary(
                *span,
                format!("`{name}` is used here, but no binding was found"),
            )),

            TypeError::UnknownConstructor { span, name } => {
                DiagnosticParts::new("TC0002", format!("unknown constructor `{name}`"))
                    .with_label(Label::primary(
                        *span,
                        format!("constructor `{name}` is not known here"),
                    ))
            }

            TypeError::UnknownPatternConstructor { span, name } => DiagnosticParts::new(
                "TC0003",
                format!("unknown constructor `{name}` in pattern"),
            )
            .with_label(Label::primary(
                *span,
                format!("pattern uses unknown constructor `{name}`"),
            )),

            TypeError::EmptyCaseExpression { span } => {
                DiagnosticParts::new("TC0004", "case expression has no arms").with_label(
                    Label::primary(*span, "this case expression needs at least one arm"),
                )
            }

            TypeError::NonExhaustivePatterns {
                span,
                scrutinee_type,
                missing,
            } => DiagnosticParts::new("TC0016", "non-exhaustive pattern match")
                .with_label(Label::primary(
                    *span,
                    format!(
                        "this case expression does not cover all values of type `{scrutinee_type}`"
                    ),
                ))
                .with_note(format_missing_patterns_note(scrutinee_type, missing)),

            TypeError::SubtypeMismatch { span, left, right } => {
                DiagnosticParts::new("TC0005", "type mismatch")
                    .with_label(Label::primary(
                        *span,
                        format!("this expression has type `{left}`"),
                    ))
                    .with_note(format!("but it was required to have type `{right}`"))
            }

            TypeError::InstantiateLeftFailed { span, evar, target } => DiagnosticParts::new(
                "TC0006",
                "failed to instantiate existential on the left side of subtyping",
            )
            .with_label(Label::primary(
                *span,
                format!("could not solve `{evar}` as a subtype of `{target}`"),
            )),

            TypeError::InstantiateRightFailed { span, source, evar } => DiagnosticParts::new(
                "TC0007",
                "failed to instantiate existential on the right side of subtyping",
            )
            .with_label(Label::primary(
                *span,
                format!("could not solve `{source}` as a subtype of `{evar}`"),
            )),

            TypeError::ConstructorNotApplicable { span, actual } => {
                DiagnosticParts::new("TC0008", "constructor is not applicable here").with_label(
                    Label::primary(
                        *span,
                        format!("expected a function-like constructor, got `{actual}`"),
                    ),
                )
            }

            TypeError::NonFunctionApplication {
                span,
                actual,
                arg_span,
            } => DiagnosticParts::new("TC0009", "application of a non-function")
                .with_label(Label::primary(
                    *span,
                    format!(
                        "this expression is applied like a function, but has type `{actual}`"
                    ),
                ))
                .with_label(Label::secondary(
                    *arg_span,
                    "unexpected argument encountered here",
                )),

            TypeError::NonPolymorphicTypeApplication {
                span,
                actual,
                ty_arg,
            } => DiagnosticParts::new(
                "TC0010",
                "type application to a non-polymorphic expression",
            )
            .with_label(Label::primary(
                *span,
                format!("this expression has type `{actual}`"),
            ))
            .with_note(format!(
                "type argument `{ty_arg}` can only be applied to a `forall` type"
            )),

            TypeError::IllFormedAnnotation { span, ty, target } => {
                DiagnosticParts::new("TC0011", "ill-formed type annotation").with_label(
                    Label::primary(
                        *span,
                        format!("annotation `{ty}` is not well-formed for {target}"),
                    ),
                )
            }

            TypeError::MissingCaseResultExistential {
                span,
                evar,
                arm_index,
            } => DiagnosticParts::new(
                "TC0012",
                "internal case-arm typing state became inconsistent",
            )
            .with_label(Label::primary(
                *span,
                format!(
                    "result existential `{evar}` was missing while checking case arm {arm_index}"
                ),
            ))
            .with_note(
                "this is reported as a user-facing type error here, but it usually points to a bug in the checker",
            ),

            TypeError::UnsatisfiedConstraints { span, predicates } => {
                let preds = predicates.join(", ");

                DiagnosticParts::new(
                    "TC0013",
                    format!("unsatisfied type class constraints: {preds}"),
                )
                .with_label(Label::primary(*span, "constraints arise from this expression"))
                .with_note(format!(
                    "no matching instance found for:\n  {}",
                    predicates.join("\n  ")
                ))
            }

            TypeError::OverlappingInstances {
                span,
                class_name,
                instance_type,
            } => DiagnosticParts::new(
                "TC0014",
                format!("overlapping instance for `{class_name} {instance_type}`"),
            )
            .with_label(Label::primary(*span, "duplicate instance defined here"))
            .with_note("an instance for this type already exists"),

            TypeError::OrphanInstance {
                span,
                class_name,
                instance_type,
            } => DiagnosticParts::warning(
                "TC0018",
                format!("orphan instance for `{class_name} {instance_type}`"),
            )
            .with_label(Label::primary(
                *span,
                "this instance is defined outside both the class and type modules",
            ))
            .with_note(
                "orphan instances are globally visible and can make instance selection less predictable",
            ),

            TypeError::UnreachablePattern { span } => {
                DiagnosticParts::warning("TC0017", "unreachable pattern").with_label(
                    Label::primary(
                        *span,
                        "this pattern is already covered by earlier case arms",
                    ),
                )
            }

            TypeError::MissingInstanceMethod {
                span,
                method,
                class_name,
            } => DiagnosticParts::new(
                "TC0019",
                format!("missing instance method `{method}` for class `{class_name}`"),
            )
            .with_label(Label::primary(
                *span,
                format!("this instance must define `{method}` or use a class default"),
            )),

            TypeError::UnsupportedFloatPattern { span } => DiagnosticParts::new(
                "TC0020",
                "float literal patterns are not supported",
            )
            .with_label(Label::primary(
                *span,
                "this pattern matches a Float literal",
            ))
            .with_note(
                "Float values are not currently allowed in literal patterns; use a wildcard and compare explicitly once Float comparisons are available",
            ),

            TypeError::InstanceMethodTypeMismatch {
                span,
                method,
                class_name,
                expected,
                cause,
            } => Self::instance_method_parts(*span, method, class_name, expected, cause),
        }
    }

    fn instance_method_parts(
        method_span: FileSpan,
        method: &str,
        class_name: &str,
        expected: &str,
        cause: &TypeError,
    ) -> DiagnosticParts {
        let cause_parts = cause.diagnostic_parts();
        let cause_anchor = cause_parts.anchor_span();

        let mut parts = DiagnosticParts::new(
            "TC0015",
            format!("type mismatch in instance method `{method}` of `{class_name}`"),
        )
        .with_labels(cause_parts.labels)
        .with_notes(cause_parts.notes);

        if cause_anchor != Some(method_span) && !method_span.is_empty() {
            parts = parts.with_label(Label::secondary(
                method_span,
                format!("while checking this implementation of `{method}`"),
            ));
        }

        parts.with_note(format!("the class requires `{expected}`"))
    }
}

impl IntoDiagnostic for TypeError {
    fn into_diagnostic(self, (): ()) -> Diagnostic {
        self.to_diagnostic()
    }
}

impl IntoDiagnostic for &TypeError {
    fn into_diagnostic(self, (): ()) -> Diagnostic {
        self.to_diagnostic()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{FileId, Span};

    #[test]
    fn sink_tracks_only_error_severity_as_errors() {
        let sink = DiagnosticSink::new();
        sink.push(Diagnostic::new("W0001", Severity::Warning, "heads up"));

        assert!(!sink.has_errors());

        sink.push(Diagnostic::error("E0001", "boom"));

        assert!(sink.has_errors());
        assert_eq!(sink.len(), 2);
    }

    #[test]
    fn sink_sorts_by_anchor_location() {
        let sink = DiagnosticSink::new();
        let file = FileId(0);
        let later = FileSpan::new(file, Span::new(10, 11));
        let earlier = FileSpan::new(file, Span::new(2, 3));

        sink.push(Diagnostic::error("LATER", "later").with_label(Label::primary(later, "later")));
        sink.push(
            Diagnostic::error("EARLIER", "earlier").with_label(Label::primary(earlier, "earlier")),
        );

        let sorted = sink.sorted_by_file_and_span();

        assert_eq!(sorted[0].code, "EARLIER");
        assert_eq!(sorted[1].code, "LATER");
    }
}
