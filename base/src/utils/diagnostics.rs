use std::cell::RefCell;
use std::error::Error;
use std::ops::Range;

use ariadne::{Label, Report, ReportKind, Source};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyFileSpan {
    pub file_id: String,
    pub span: Range<usize>,
}

impl LegacyFileSpan {
    pub fn new(file_id: impl Into<String>, span: Range<usize>) -> Self {
        Self {
            file_id: file_id.into(),
            span,
        }
    }

    pub fn range(&self) -> Range<usize> {
        self.span.clone()
    }

    pub fn is_empty(&self) -> bool {
        self.span.start == self.span.end
    }
}

#[derive(Debug, Default)]
pub struct DiagnosticSink {
    diagnostics: RefCell<Vec<CompilerDiagnostic>>,
}

impl DiagnosticSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, diagnostic: CompilerDiagnostic) {
        self.diagnostics.borrow_mut().push(diagnostic);
    }

    pub fn has_errors(&self) -> bool {
        !self.diagnostics.borrow().is_empty()
    }

    pub fn drain(&self) -> Vec<CompilerDiagnostic> {
        self.diagnostics.borrow_mut().drain(..).collect()
    }

    pub fn sorted_by_file_and_span(&self) -> Vec<CompilerDiagnostic> {
        let mut diagnostics = self.diagnostics.borrow().clone();

        diagnostics.sort_by(|left, right| {
            let left_anchor = left.anchor_span();
            let right_anchor = right.anchor_span();

            match (left_anchor, right_anchor) {
                (Some(left), Some(right)) => left
                    .file_id
                    .cmp(&right.file_id)
                    .then(left.span.start.cmp(&right.span.start))
                    .then(left.span.end.cmp(&right.span.end)),
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, None) => left.code.cmp(right.code).then(left.message.cmp(&right.message)),
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
pub struct DiagnosticLabel {
    pub span: LegacyFileSpan,
    pub message: String,
    pub primary: bool,
}

impl DiagnosticLabel {
    pub fn primary(span: LegacyFileSpan, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            primary: true,
        }
    }

    pub fn secondary(span: LegacyFileSpan, message: impl Into<String>) -> Self {
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

    pub fn with_notes<I, S>(mut self, notes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.notes.extend(notes.into_iter().map(Into::into));
        self
    }

    pub fn anchor_span(&self) -> Option<&LegacyFileSpan> {
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
        span: LegacyFileSpan,
        name: String,
    },
    UnknownConstructor {
        span: LegacyFileSpan,
        name: String,
    },
    UnknownPatternConstructor {
        span: LegacyFileSpan,
        name: String,
    },
    EmptyCaseExpression {
        span: LegacyFileSpan,
    },
    SubtypeMismatch {
        span: LegacyFileSpan,
        left: String,
        right: String,
    },
    InstantiateLeftFailed {
        span: LegacyFileSpan,
        evar: String,
        target: String,
    },
    InstantiateRightFailed {
        span: LegacyFileSpan,
        source: String,
        evar: String,
    },
    ConstructorNotApplicable {
        span: LegacyFileSpan,
        actual: String,
    },
    NonFunctionApplication {
        span: LegacyFileSpan,
        arg_span: LegacyFileSpan,
        actual: String,
    },
    NonPolymorphicTypeApplication {
        span: LegacyFileSpan,
        actual: String,
        ty_arg: String,
    },
    IllFormedAnnotation {
        span: LegacyFileSpan,
        ty: String,
        target: String,
    },
    MissingCaseResultExistential {
        span: LegacyFileSpan,
        evar: String,
        arm_index: usize,
    },
    UnsatisfiedConstraints {
        span: LegacyFileSpan,
        predicates: Vec<String>,
    },
    OverlappingInstances {
        span: LegacyFileSpan,
        class_name: String,
        instance_type: String,
    },

    /// Contextual wrapper around the real error that happened while checking an
    /// instance method body against the specialized class method type.
    ///
    /// Do not pre-render the inner error into a string here. Keeping the cause
    /// structured lets diagnostics underline the precise expression that failed
    /// and add instance/class context as notes.
    InstanceMethodTypeMismatch {
        span: LegacyFileSpan,
        method: String,
        class_name: String,
        expected: String,
        cause: Box<TypeError>,
    },
    NonExhaustivePatterns {
        span: LegacyFileSpan,
        scrutinee_type: String,
        missing: Vec<String>,
    },
}

fn format_missing_patterns_note(scrutinee_type: &str, missing: &[String]) -> String {
    match missing {
        [] => format!(
            "scrutinee type: `{}`\nmissing pattern:\n  _",
            scrutinee_type
        ),
        [one] => format!(
            "scrutinee type: `{}`\nmissing pattern:\n  {}",
            scrutinee_type, one
        ),
        many => format!(
            "scrutinee type: `{}`\nmissing pattern examples:\n  {}",
            scrutinee_type,
            many.join("\n  ")
        ),
    }
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
            TypeError::NonExhaustivePatterns {
                span,
                scrutinee_type,
                missing,
            } => CompilerDiagnostic::new("TC0016", "non-exhaustive pattern match")
                .with_label(DiagnosticLabel::primary(
                    span.clone(),
                    format!(
                        "this case expression does not cover all values of type `{}`",
                        scrutinee_type
                    ),
                ))
                .with_note(format_missing_patterns_note(scrutinee_type, missing)),

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

            TypeError::NonFunctionApplication {
                span,
                actual,
                arg_span,
            } => CompilerDiagnostic::new("TC0009", "application of a non-function")
                .with_label(DiagnosticLabel::primary(
                    span.clone(),
                    format!(
                        "this expression is applied like a function, but has type `{}`",
                        actual
                    ),
                ))
                .with_label(DiagnosticLabel::secondary(
                    arg_span.clone(),
                    "unexpected argument encountered here",
                )),

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
                let preds = predicates.join(", ");

                CompilerDiagnostic::new(
                    "TC0013",
                    format!("unsatisfied type class constraints: {}", preds),
                )
                .with_label(DiagnosticLabel::primary(
                    span.clone(),
                    "constraints arise from this expression",
                ))
                .with_note(format!(
                    "no matching instance found for:\n  {}",
                    predicates.join("\n  ")
                ))
            }

            TypeError::OverlappingInstances {
                span,
                class_name,
                instance_type,
            } => CompilerDiagnostic::new(
                "TC0014",
                format!("overlapping instance for `{} {}`", class_name, instance_type),
            )
            .with_label(DiagnosticLabel::primary(
                span.clone(),
                "duplicate instance defined here",
            ))
            .with_note("an instance for this type already exists"),

            TypeError::InstanceMethodTypeMismatch {
                span,
                method,
                class_name,
                expected,
                cause,
            } => Self::instance_method_diagnostic(span, method, class_name, expected, cause),
        }
    }

    fn instance_method_diagnostic(
        method_span: &LegacyFileSpan,
        method: &str,
        class_name: &str,
        expected: &str,
        cause: &TypeError,
    ) -> CompilerDiagnostic {
        let mut diagnostic = CompilerDiagnostic::new(
            "TC0015",
            format!(
                "type mismatch in instance method `{}` of `{}`",
                method, class_name
            ),
        );

        for label in cause.primary_labels_for_context() {
            diagnostic = diagnostic.with_label(label);
        }

        if !span_same_location(method_span, cause.primary_span()) && !method_span.is_empty() {
            diagnostic = diagnostic.with_label(DiagnosticLabel::secondary(
                method_span.clone(),
                format!("while checking this implementation of `{}`", method),
            ));
        }

        diagnostic
            .with_notes(cause.notes_for_context())
            .with_note(format!("the class requires `{}`", expected))
    }

    fn primary_span(&self) -> &LegacyFileSpan {
        match self {
            TypeError::UnboundVariable { span, .. }
            | TypeError::UnknownConstructor { span, .. }
            | TypeError::UnknownPatternConstructor { span, .. }
            | TypeError::EmptyCaseExpression { span }
            | TypeError::NonExhaustivePatterns { span, .. }
            | TypeError::SubtypeMismatch { span, .. }
            | TypeError::InstantiateLeftFailed { span, .. }
            | TypeError::InstantiateRightFailed { span, .. }
            | TypeError::ConstructorNotApplicable { span, .. }
            | TypeError::NonFunctionApplication { span, .. }
            | TypeError::NonPolymorphicTypeApplication { span, .. }
            | TypeError::IllFormedAnnotation { span, .. }
            | TypeError::MissingCaseResultExistential { span, .. }
            | TypeError::UnsatisfiedConstraints { span, .. }
            | TypeError::OverlappingInstances { span, .. } => span,

            TypeError::InstanceMethodTypeMismatch { cause, .. } => cause.primary_span(),
        }
    }

    fn primary_labels_for_context(&self) -> Vec<DiagnosticLabel> {
        match self {
            TypeError::UnboundVariable { span, name } => vec![DiagnosticLabel::primary(
                span.clone(),
                format!("`{}` is used here, but no binding was found", name),
            )],

            TypeError::UnknownConstructor { span, name } => vec![DiagnosticLabel::primary(
                span.clone(),
                format!("constructor `{}` is not known here", name),
            )],

            TypeError::UnknownPatternConstructor { span, name } => {
                vec![DiagnosticLabel::primary(
                    span.clone(),
                    format!("pattern uses unknown constructor `{}`", name),
                )]
            }

            TypeError::EmptyCaseExpression { span } => vec![DiagnosticLabel::primary(
                span.clone(),
                "this case expression needs at least one arm",
            )],
            TypeError::NonExhaustivePatterns { span, .. } => vec![DiagnosticLabel::primary(
                span.clone(),
                "this case expression does not cover all possible inputs",
            )],

            TypeError::SubtypeMismatch { span, left, .. } => vec![DiagnosticLabel::primary(
                span.clone(),
                format!("this expression has type `{}`", left),
            )],

            TypeError::InstantiateLeftFailed { span, evar, target } => {
                vec![DiagnosticLabel::primary(
                    span.clone(),
                    format!("could not solve `{}` as a subtype of `{}`", evar, target),
                )]
            }

            TypeError::InstantiateRightFailed { span, source, evar } => {
                vec![DiagnosticLabel::primary(
                    span.clone(),
                    format!("could not solve `{}` as a subtype of `{}`", source, evar),
                )]
            }

            TypeError::ConstructorNotApplicable { span, actual } => {
                vec![DiagnosticLabel::primary(
                    span.clone(),
                    format!("expected a function-like constructor, got `{}`", actual),
                )]
            }

            TypeError::NonFunctionApplication {
                span,
                arg_span,
                actual,
            } => vec![
                DiagnosticLabel::primary(
                    span.clone(),
                    format!(
                        "this expression is applied like a function, but has type `{}`",
                        actual
                    ),
                ),
                DiagnosticLabel::secondary(
                    arg_span.clone(),
                    "unexpected argument encountered here",
                ),
            ],

            TypeError::NonPolymorphicTypeApplication { span, actual, .. } => {
                vec![DiagnosticLabel::primary(
                    span.clone(),
                    format!("this expression has type `{}`", actual),
                )]
            }

            TypeError::IllFormedAnnotation { span, ty, target } => {
                vec![DiagnosticLabel::primary(
                    span.clone(),
                    format!("annotation `{}` is not well-formed for {}", ty, target),
                )]
            }

            TypeError::MissingCaseResultExistential {
                span,
                evar,
                arm_index,
            } => vec![DiagnosticLabel::primary(
                span.clone(),
                format!(
                    "result existential `{}` was missing while checking case arm {}",
                    evar, arm_index
                ),
            )],

            TypeError::UnsatisfiedConstraints { span, .. } => vec![DiagnosticLabel::primary(
                span.clone(),
                "constraints arise from this expression",
            )],

            TypeError::OverlappingInstances { span, .. } => vec![DiagnosticLabel::primary(
                span.clone(),
                "duplicate instance defined here",
            )],

            TypeError::InstanceMethodTypeMismatch { cause, .. } => {
                cause.primary_labels_for_context()
            }
        }
    }

    fn notes_for_context(&self) -> Vec<String> {
        match self {
            TypeError::SubtypeMismatch { right, .. } => {
                vec![format!("but it was required to have type `{}`", right)]
            }

            TypeError::NonPolymorphicTypeApplication { ty_arg, .. } => vec![format!(
                "type argument `{}` can only be applied to a `forall` type",
                ty_arg
            )],

            TypeError::MissingCaseResultExistential { .. } => vec![
                "this is reported as a user-facing type error here, but it usually points to a bug in the checker".to_string(),
            ],

            TypeError::UnsatisfiedConstraints { predicates, .. } => vec![format!(
                "no matching instance found for:\n  {}",
                predicates.join("\n  ")
            )],

            TypeError::OverlappingInstances { .. } => {
                vec!["an instance for this type already exists".to_string()]
            }

            TypeError::InstanceMethodTypeMismatch {
                method,
                class_name,
                expected,
                cause,
                ..
            } => {
                let mut notes = cause.notes_for_context();
                notes.push(format!(
                    "while checking instance method `{}` of `{}`",
                    method, class_name
                ));
                notes.push(format!("the class requires `{}`", expected));
                notes
            }
            TypeError::NonExhaustivePatterns {
                    scrutinee_type,
                    missing,
                    ..
                } => vec![format_missing_patterns_note(scrutinee_type, missing)],

            _ => Vec::new(),
        }
    }
}

fn span_same_location(a: &LegacyFileSpan, b: &LegacyFileSpan) -> bool {
    a.file_id == b.file_id && a.span == b.span
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
