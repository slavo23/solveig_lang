use std::error::Error;
use std::ops::Range;

use crate::utils::span::Span;
use ariadne::{Label, Report, ReportKind, Source};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSpan {
    pub file_id: String,
    pub span: Span,
}

impl FileSpan {
    pub fn new(file_id: impl Into<String>, span: Span) -> Self {
        Self {
            file_id: file_id.into(),
            span,
        }
    }

    pub fn range(&self) -> Range<usize> {
        self.span.clone()
    }
}

#[derive(Clone, Debug)]
pub struct DiagnosticLabel {
    pub span: FileSpan,
    pub message: String,
    pub primary: bool,
}

impl DiagnosticLabel {
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

#[derive(Clone, Debug)]
pub struct CompilerDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub labels: Vec<DiagnosticLabel>,
    pub notes: Vec<String>,
}

impl CompilerDiagnostic {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn with_label(mut self, label: DiagnosticLabel) -> Self {
        self.labels.push(label);
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
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
}

impl TypeError {
    pub fn to_diagnostic(&self) -> CompilerDiagnostic {
        match self {
            TypeError::UnboundVariable { span, name } => CompilerDiagnostic::new(
                "TC0001",
                format!("variable `{}` is not in scope", name),
            )
            .with_label(DiagnosticLabel::primary(
                span.clone(),
                format!("`{}` is used here, but no binding was found", name),
            )),

            TypeError::UnknownConstructor { span, name } => CompilerDiagnostic::new(
                "TC0002",
                format!("unknown constructor `{}`", name),
            )
            .with_label(DiagnosticLabel::primary(
                span.clone(),
                format!("constructor `{}` is not known here", name),
            )),

            TypeError::UnknownPatternConstructor { span, name } => CompilerDiagnostic::new(
                "TC0003",
                format!("unknown constructor `{}` in pattern", name),
            )
            .with_label(DiagnosticLabel::primary(
                span.clone(),
                format!("pattern uses unknown constructor `{}`", name),
            )),

            TypeError::EmptyCaseExpression { span } => CompilerDiagnostic::new(
                "TC0004",
                "case expression has no arms",
            )
            .with_label(DiagnosticLabel::primary(
                span.clone(),
                "this case expression needs at least one arm",
            )),

            TypeError::SubtypeMismatch { span, left, right } => CompilerDiagnostic::new(
                "TC0005",
                "type mismatch",
            )
            .with_label(DiagnosticLabel::primary(
                span.clone(),
                format!("this expression has type `{}`", left),
            ))
            .with_note(format!("but it was required to have type `{}`", right)),

            TypeError::InstantiateLeftFailed { span, evar, target } => CompilerDiagnostic::new(
                "TC0006",
                "failed to instantiate existential on the left side of subtyping",
            )
            .with_label(DiagnosticLabel::primary(
                span.clone(),
                format!("could not solve `{}` as a subtype of `{}`", evar, target),
            )),

            TypeError::InstantiateRightFailed { span, source, evar } => CompilerDiagnostic::new(
                "TC0007",
                "failed to instantiate existential on the right side of subtyping",
            )
            .with_label(DiagnosticLabel::primary(
                span.clone(),
                format!("could not solve `{}` as a subtype of `{}`", source, evar),
            )),

            TypeError::ConstructorNotApplicable { span, actual } => CompilerDiagnostic::new(
                "TC0008",
                "constructor is not applicable here",
            )
            .with_label(DiagnosticLabel::primary(
                span.clone(),
                format!("expected a function-like constructor, got `{}`", actual),
            )),

            TypeError::NonFunctionApplication { span, actual, arg_span } => CompilerDiagnostic::new(
                "TC0009",
                "application of a non-function",
            )
            .with_label(DiagnosticLabel::primary(
                span.clone(),
                format!("this expression is applied like a function, but has type `{}`", actual),
            )).with_label(DiagnosticLabel::secondary(arg_span.clone(), "unexpected argument encountered here".to_string()))
            ,

            TypeError::NonPolymorphicTypeApplication {
                span,
                actual,
                ty_arg,
            } => CompilerDiagnostic::new(
                "TC0010",
                "type application to a non-polymorphic expression",
            )
            .with_label(DiagnosticLabel::primary(
                span.clone(),
                format!("this expression has type `{}`", actual),
            ))
            .with_note(format!(
                "type argument `{}` can only be applied to a `forall` type",
                ty_arg
            )),

            TypeError::IllFormedAnnotation { span, ty, target } => CompilerDiagnostic::new(
                "TC0011",
                "ill-formed type annotation",
            )
            .with_label(DiagnosticLabel::primary(
                span.clone(),
                format!("annotation `{}` is not well-formed for {}", ty, target),
            )),

            TypeError::MissingCaseResultExistential {
                span,
                evar,
                arm_index,
            } => CompilerDiagnostic::new(
                "TC0012",
                "internal case-arm typing state became inconsistent",
            )
            .with_label(DiagnosticLabel::primary(
                span.clone(),
                format!(
                    "result existential `{}` was missing while checking case arm {}",
                    evar, arm_index
                ),
            ))
            .with_note(
                "this is reported as a user-facing type error here, but it usually points to a bug in the checker",
            ),
            TypeError::UnsatisfiedConstraints { span, predicates } => {
                let summary = if predicates.len() == 1 {
                    format!("unsatisfied constraint `{}`", predicates[0])
                } else {
                    "unsatisfied constraints".to_string()
                };

                let details = predicates
                    .iter()
                    .map(|p| format!("`{}`", p))
                    .collect::<Vec<_>>()
                    .join(", ");

                CompilerDiagnostic::new("TC0013", summary)
                    .with_label(DiagnosticLabel::primary(
                        span.clone(),
                        "this expression requires class evidence that could not be found",
                    ))
                    .with_note(format!("missing evidence for {}", details))
                    .with_note(
                        "try adding the required constraint to the surrounding signature or defining a matching instance",
                    )
            }
        }
    }
}

pub fn emit_diagnostic(diag: &CompilerDiagnostic, src: &str) -> Result<(), Box<dyn Error>> {
    let anchor = diag
        .anchor_span()
        .ok_or("diagnostic has no labels and therefore no anchor span")?;

    let mut builder = Report::build(ReportKind::Error, (anchor.file_id.clone(), anchor.range()))
        .with_code(diag.code)
        .with_message(diag.message.clone());

    for label in &diag.labels {
        let ariadne_label = Label::new((label.span.file_id.clone(), label.span.range()))
            .with_message(label.message.clone());

        builder = builder.with_label(ariadne_label);
    }

    for note in &diag.notes {
        builder = builder.with_note(note.clone());
    }

    builder
        .finish()
        .eprint((anchor.file_id.clone(), Source::from(src.to_string())))?;

    Ok(())
}

pub fn emit_type_errors(errors: &[TypeError], src: &str) -> Result<(), Box<dyn Error>> {
    for err in errors {
        emit_diagnostic(&err.to_diagnostic(), src)?;
    }
    Ok(())
}
