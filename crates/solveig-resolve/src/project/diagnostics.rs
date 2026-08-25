//! Structured resolver diagnostics.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveNameKind {
    Term,
    Operator,
    TypeConstructor,
    TypeAlias,
    Constructor,
    Class,
    Signature,
    Fixity,
}

impl ResolveNameKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Term => "term",
            Self::Operator => "operator",
            Self::TypeConstructor => "type constructor",
            Self::TypeAlias => "type alias",
            Self::Constructor => "constructor",
            Self::Class => "class",
            Self::Signature => "signature",
            Self::Fixity => "fixity",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ResolveError {
    AmbiguousImport {
        span: FileSpan,
        kind: ResolveNameKind,
        name: Symbol,
        candidates: Vec<DefId>,
    },
    UnknownModule {
        span: FileSpan,
        module: ModuleName,
    },
    UnknownImportName {
        span: FileSpan,
        module: ModuleName,
        name: Symbol,
    },
    AmbiguousModuleQualifier {
        span: FileSpan,
        qualifier: Symbol,
        modules: Vec<ModuleName>,
    },
    InvalidModuleDirective {
        span: FileSpan,
        message: String,
    },
    ReexportConflict {
        span: FileSpan,
        name: Symbol,
    },
    UnknownName {
        span: FileSpan,
        kind: ResolveNameKind,
        name: String,
    },
    DuplicateDefinition {
        span: FileSpan,
        previous: Option<FileSpan>,
        kind: ResolveNameKind,
        name: Symbol,
    },
    EquationArityMismatch {
        span: FileSpan,
        previous: FileSpan,
        name: Symbol,
        expected: usize,
        actual: usize,
    },
    NonContiguousEquationGroup {
        span: FileSpan,
        previous: FileSpan,
        name: Symbol,
    },
    RepeatedPatternVariable {
        span: FileSpan,
        name: Symbol,
    },
    ClassArity {
        span: FileSpan,
        class_name: String,
        actual: usize,
    },
    SignatureArity {
        span: FileSpan,
        name: Symbol,
        params: usize,
    },
    FixityConflict {
        span: FileSpan,
        left: Symbol,
        right: Symbol,
    },
    IllegalUnaryMinus {
        span: FileSpan,
    },
    MalformedOperatorExpression {
        span: FileSpan,
        message: String,
    },
    CyclicImports {
        span: FileSpan,
        modules: Vec<ModuleName>,
    },
    TypeAliasArity {
        span: FileSpan,
        name: Symbol,
        expected: usize,
        actual: usize,
    },
    PartialTypeAlias {
        span: FileSpan,
        name: Symbol,
        expected: usize,
        actual: usize,
    },
    CyclicTypeAlias {
        span: FileSpan,
        names: Vec<Symbol>,
    },
    OrphanInstance {
        span: FileSpan,
        class_name: String,
        instance_type: String,
    },
    OverlappingInstances {
        span: FileSpan,
        previous: FileSpan,
        class_name: String,
        instance_type: String,
    },
}

impl ResolveError {
    pub fn to_diagnostic(&self, session: &Session, modules: Option<&ModuleTable>) -> Diagnostic {
        match self {
            Self::AmbiguousImport {
                span,
                kind,
                name,
                candidates,
            } => {
                let mut notes = Vec::new();
                if let Some(modules) = modules {
                    let rendered = candidates
                        .iter()
                        .map(|candidate| modules.def_path(*candidate).render(session))
                        .collect::<Vec<_>>()
                        .join(", ");

                    if !rendered.is_empty() {
                        notes.push(format!("candidates: {rendered}"));
                    }
                }

                Diagnostic::error(
                    "RES0001",
                    format!(
                        "ambiguous imported {} `{}`",
                        kind.as_str(),
                        session.str(*name)
                    ),
                )
                .with_label(Label::primary(
                    *span,
                    format!(
                        "`{}` is provided by more than one imported module",
                        session.str(*name)
                    ),
                ))
                .with_notes(notes)
            }
            Self::UnknownModule { span, module } => Diagnostic::error(
                "RES0002",
                format!("unknown module `{}`", module.as_string(session)),
            )
            .with_label(Label::primary(
                *span,
                format!("module `{}` could not be found", module.as_string(session)),
            )),
            Self::UnknownImportName { span, module, name } => Diagnostic::error(
                "RES0014",
                format!(
                    "module `{}` does not export `{}`",
                    module.as_string(session),
                    session.str(*name)
                ),
            )
            .with_label(Label::primary(
                *span,
                format!("`{}` is not exported by this module", session.str(*name)),
            )),
            Self::AmbiguousModuleQualifier {
                span,
                qualifier,
                modules,
            } => Diagnostic::error(
                "RES0020",
                format!(
                    "ambiguous module qualifier `{}`",
                    session.str(*qualifier)
                ),
            )
            .with_label(Label::primary(
                *span,
                format!(
                    "`{}` already qualifies another imported module",
                    session.str(*qualifier)
                ),
            ))
            .with_notes(vec![format!(
                "conflicting modules: {}",
                modules
                    .iter()
                    .map(|module| module.as_string(session))
                    .collect::<Vec<_>>()
                    .join(", ")
            )]),
            Self::InvalidModuleDirective { span, message } => {
                Diagnostic::error("RES0019", message.clone())
                    .with_label(Label::primary(*span, message.clone()))
            }
            Self::ReexportConflict { span, name } => Diagnostic::error(
                "RES0019",
                format!("conflicting re-export `{}`", session.str(*name)),
            )
            .with_label(Label::primary(
                *span,
                format!(
                    "`{}` is already provided by another visible definition",
                    session.str(*name)
                ),
            )),
            Self::UnknownName { span, kind, name } => {
                Diagnostic::error("RES0003", format!("unknown {} `{}`", kind.as_str(), name))
                    .with_label(Label::primary(
                        *span,
                        format!("no {} named `{}` is in scope here", kind.as_str(), name),
                    ))
            }
            Self::DuplicateDefinition {
                span,
                previous,
                kind,
                name,
            } => {
                let mut diagnostic = Diagnostic::error(
                    "RES0004",
                    format!(
                        "duplicate {} definition `{}`",
                        kind.as_str(),
                        session.str(*name)
                    ),
                )
                .with_label(Label::primary(
                    *span,
                    format!("`{}` is defined again here", session.str(*name)),
                ));

                if let Some(previous) = previous {
                    diagnostic = diagnostic.with_label(Label::secondary(
                        *previous,
                        format!("previous definition of `{}` is here", session.str(*name)),
                    ));
                }

                diagnostic
            }
            Self::EquationArityMismatch {
                span,
                previous,
                name,
                expected,
                actual,
            } => Diagnostic::error(
                "RES0015",
                format!(
                    "equations for `{}` have different numbers of arguments",
                    session.str(*name)
                ),
            )
            .with_label(Label::primary(
                *span,
                format!(
                    "this equation has {actual} argument{}, but the earlier equation has {expected}",
                    plural(*actual)
                ),
            ))
            .with_label(Label::secondary(
                *previous,
                format!("earlier equation for `{}` is here", session.str(*name)),
            )),
            Self::NonContiguousEquationGroup {
                span,
                previous,
                name,
            } => Diagnostic::error(
                "RES0016",
                format!("equations for `{}` must be adjacent", session.str(*name)),
            )
            .with_label(Label::primary(
                *span,
                "this equation is separated from the earlier equations",
            ))
            .with_label(Label::secondary(
                *previous,
                format!("earlier equation for `{}` starts here", session.str(*name)),
            ))
            .with_notes(vec![
                "move all equations for the same function next to each other".to_string(),
            ]),
            Self::RepeatedPatternVariable { span, name } => Diagnostic::error(
                "RES0017",
                format!("pattern variable `{}` appears more than once", session.str(*name)),
            )
            .with_label(Label::primary(
                *span,
                "this variable is already bound earlier in the same pattern group",
            ))
            .with_notes(vec![
                "use distinct variable names and compare them in the body if equality is required"
                    .to_string(),
            ]),
            Self::ClassArity {
                span,
                class_name,
                actual,
            } => Diagnostic::error(
                "RES0005",
                format!("class `{class_name}` must have exactly one parameter"),
            )
            .with_label(Label::primary(
                *span,
                format!("expected exactly one class parameter, but found {actual}"),
            )),
            Self::SignatureArity { span, name, params } => Diagnostic::error(
                "RES0006",
                format!(
                    "function `{}` has more parameters than its signature accepts",
                    session.str(*name)
                ),
            )
            .with_label(Label::primary(
                *span,
                format!(
                    "this definition has {} parameter{}, but the signature is not a function type",
                    params,
                    if *params == 1 { "" } else { "s" }
                ),
            )),
            Self::FixityConflict { span, left, right } => Diagnostic::error(
                "RES0008",
                "conflicting operator fixities",
            )
            .with_label(Label::primary(
                *span,
                format!(
                    "operators `{}` and `{}` have the same precedence but cannot be mixed without parentheses",
                    session.str(*left),
                    session.str(*right)
                ),
            )),
            Self::IllegalUnaryMinus { span } => Diagnostic::error(
                "RES0009",
                "illegal unary minus",
            )
            .with_label(Label::primary(
                *span,
                "prefix `-` is not allowed as the right operand of this operator",
            ))
            .with_notes(vec![
                "add parentheses if the negative expression is intended here".to_string(),
            ]),
            Self::MalformedOperatorExpression { span, message } => Diagnostic::error(
                "RES0010",
                "malformed operator expression",
            )
            .with_label(Label::primary(*span, message.clone())),
            Self::CyclicImports { span, modules } => {
                Diagnostic::error("RES0007", "cyclic module imports").with_label(Label::primary(
                    *span,
                    format!(
                        "cycle: {}",
                        modules
                            .iter()
                            .map(|module| module.as_string(session))
                            .collect::<Vec<_>>()
                            .join(" -> ")
                    ),
                ))
            }
            Self::TypeAliasArity {
                span,
                name,
                expected,
                actual,
            } => Diagnostic::error(
                "RES0011",
                format!("type alias `{}` has the wrong number of arguments", session.str(*name)),
            )
            .with_label(Label::primary(
                *span,
                format!("expected {expected} argument{}, but found {actual}", plural(*expected)),
            )),
            Self::PartialTypeAlias {
                span,
                name,
                expected,
                actual,
            } => Diagnostic::error(
                "RES0012",
                format!("type alias `{}` is partially applied", session.str(*name)),
            )
            .with_label(Label::primary(
                *span,
                format!(
                    "type aliases must be fully applied; expected {expected} argument{}, but found {actual}",
                    plural(*expected)
                ),
            )),
            Self::CyclicTypeAlias { span, names } => Diagnostic::error(
                "RES0013",
                "cyclic type alias",
            )
            .with_label(Label::primary(*span, "this alias expands through itself"))
            .with_notes(vec![format!(
                "cycle: {}",
                names
                    .iter()
                    .map(|name| session.str(*name).to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ")
            )]),
            Self::OrphanInstance {
                span,
                class_name,
                instance_type,
            } => Diagnostic::error(
                "RES0021",
                format!("orphan instance for `{class_name} {instance_type}`"),
            )
            .with_label(Label::primary(
                *span,
                "this package owns neither the class nor the outer type constructor",
            ))
            .with_notes(vec![
                "define the instance in the package that owns the class or the type".to_string(),
            ]),
            Self::OverlappingInstances {
                span,
                previous,
                class_name,
                instance_type,
            } => Diagnostic::error(
                "RES0022",
                format!("overlapping instance for `{class_name} {instance_type}`"),
            )
            .with_label(Label::primary(*span, "this instance overlaps an existing instance"))
            .with_label(Label::secondary(*previous, "the other instance is defined here"))
            .with_notes(vec![
                "every visible package graph must contain at most one matching instance"
                    .to_string(),
            ]),
        }
    }
}

impl<'a> IntoDiagnostic<(&'a Session, Option<&'a ModuleTable>)> for ResolveError {
    fn into_diagnostic(
        self,
        (session, modules): (&'a Session, Option<&'a ModuleTable>),
    ) -> Diagnostic {
        self.to_diagnostic(session, modules)
    }
}

impl<'a> IntoDiagnostic<(&'a Session, Option<&'a ModuleTable>)> for &ResolveError {
    fn into_diagnostic(
        self,
        (session, modules): (&'a Session, Option<&'a ModuleTable>),
    ) -> Diagnostic {
        self.to_diagnostic(session, modules)
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
