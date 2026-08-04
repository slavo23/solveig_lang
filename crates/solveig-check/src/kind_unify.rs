//! Shared HM kind unification for data declarations, signatures, and classes.
//! Following Haskell 2010 Report 4.6 and THIH 11, unsolved kind variables
//! default to `*` after inference. `ena` is used for one union-find
//! implementation and path compression; kind graphs are tiny, so this is about
//! owning fewer algorithms, not speed.

use ena::unify::{InPlaceUnificationTable, UnifyKey, UnifyValue};
use solveig_ast::ast::{Kind, KindMetaId, Pretty};
use solveig_base::{Diagnostic, FileSpan, FxHashMap, IntoDiagnostic, Label};

#[derive(Debug, Clone)]
pub enum KindError {
    UnboundTypeVariable {
        span: Option<FileSpan>,
        name: String,
    },
    UnboundTypeConstructor {
        span: Option<FileSpan>,
        name: String,
    },
    OccursCheckFailed {
        span: Option<FileSpan>,
        var: KindMetaId,
        in_kind: Kind,
    },
    CannotUnify {
        span: Option<FileSpan>,
        lhs: Kind,
        rhs: Kind,
    },
    SignatureMustHaveKindStar {
        span: Option<FileSpan>,
        got: Kind,
    },
}

impl KindError {
    pub fn unbound_type_variable(name: impl Into<String>) -> Self {
        Self::UnboundTypeVariable {
            span: None,
            name: name.into(),
        }
    }

    pub fn unbound_type_constructor(name: impl Into<String>) -> Self {
        Self::UnboundTypeConstructor {
            span: None,
            name: name.into(),
        }
    }

    pub fn occurs_check_failed(var: KindMetaId, in_kind: Kind) -> Self {
        Self::OccursCheckFailed {
            span: None,
            var,
            in_kind,
        }
    }

    pub fn cannot_unify(lhs: Kind, rhs: Kind) -> Self {
        Self::CannotUnify {
            span: None,
            lhs,
            rhs,
        }
    }

    pub fn signature_must_have_kind_star(got: Kind) -> Self {
        Self::SignatureMustHaveKindStar { span: None, got }
    }

    pub fn with_span(mut self, span: FileSpan) -> Self {
        match &mut self {
            Self::UnboundTypeVariable {
                span: error_span, ..
            }
            | Self::UnboundTypeConstructor {
                span: error_span, ..
            }
            | Self::OccursCheckFailed {
                span: error_span, ..
            }
            | Self::CannotUnify {
                span: error_span, ..
            }
            | Self::SignatureMustHaveKindStar {
                span: error_span, ..
            } => {
                if error_span.is_none() {
                    *error_span = Some(span);
                }
            }
        }

        self
    }

    pub fn to_diagnostic(&self) -> Diagnostic {
        match self {
            Self::UnboundTypeVariable { span, name } => {
                let mut diagnostic = Diagnostic::error(
                    "KIND0001",
                    format!("type variable `{name}` is not in scope"),
                );
                if let Some(span) = span {
                    diagnostic = diagnostic.with_label(Label::primary(
                        *span,
                        format!("`{name}` is used here, but no type variable was bound"),
                    ));
                }
                diagnostic
            }

            Self::UnboundTypeConstructor { span, name } => {
                let mut diagnostic = Diagnostic::error(
                    "KIND0002",
                    format!("type constructor `{name}` is not in scope"),
                );
                if let Some(span) = span {
                    diagnostic = diagnostic.with_label(Label::primary(
                        *span,
                        format!("`{name}` is used here, but no type constructor was found"),
                    ));
                }
                diagnostic
            }

            Self::OccursCheckFailed { span, var, in_kind } => {
                let var = Kind::make_meta(*var).pretty();
                let mut diagnostic = Diagnostic::error(
                    "KIND0003",
                    format!(
                        "cannot construct recursive kind `{}` in `{}`",
                        var,
                        in_kind.pretty()
                    ),
                );
                if let Some(span) = span {
                    diagnostic = diagnostic.with_label(Label::primary(
                        *span,
                        format!(
                            "kind variable `{}` occurs inside `{}`",
                            var,
                            in_kind.pretty()
                        ),
                    ));
                }
                diagnostic
            }

            Self::CannotUnify { span, lhs, rhs } => {
                let mut diagnostic = Diagnostic::error(
                    "KIND0004",
                    format!(
                        "kind mismatch: `{}` is not `{}`",
                        lhs.pretty(),
                        rhs.pretty()
                    ),
                );
                if let Some(span) = span {
                    diagnostic = diagnostic.with_label(Label::primary(
                        *span,
                        format!(
                            "this type has kind `{}`, but kind `{}` was required",
                            lhs.pretty(),
                            rhs.pretty()
                        ),
                    ));
                }
                diagnostic
            }

            Self::SignatureMustHaveKindStar { span, got } => {
                let mut diagnostic = Diagnostic::error(
                    "KIND0005",
                    format!("signature has kind `{}` instead of `*`", got.pretty()),
                );
                if let Some(span) = span {
                    diagnostic = diagnostic.with_label(Label::primary(
                        *span,
                        format!("this signature has kind `{}`", got.pretty()),
                    ));
                }
                diagnostic
            }
        }
    }
}

impl IntoDiagnostic for KindError {
    fn into_diagnostic(self, (): ()) -> Diagnostic {
        self.to_diagnostic()
    }
}

impl IntoDiagnostic for &KindError {
    fn into_diagnostic(self, (): ()) -> Diagnostic {
        self.to_diagnostic()
    }
}

pub struct KindUnifier {
    table: InPlaceUnificationTable<KindVar>,
    // Public kind metas can be sparse because resolver seed placeholders may
    // use high ids. `ena` keys stay dense and private to this table.
    meta_vars: FxHashMap<KindMetaId, KindVar>,
    var_metas: Vec<KindMetaId>,
    next_meta: u32,
}

impl Default for KindUnifier {
    fn default() -> Self {
        Self::new()
    }
}

impl KindUnifier {
    pub fn new() -> Self {
        Self {
            table: InPlaceUnificationTable::new(),
            meta_vars: FxHashMap::default(),
            var_metas: Vec::new(),
            next_meta: 0,
        }
    }

    pub fn fresh(&mut self) -> Kind {
        while self.meta_vars.contains_key(&KindMetaId(self.next_meta)) {
            self.next_meta += 1;
        }
        let id = KindMetaId(self.next_meta);
        self.next_meta += 1;
        self.insert_meta(id);
        Kind::make_meta(id)
    }

    pub fn unify(&mut self, lhs: &Kind, rhs: &Kind) -> Result<(), KindError> {
        let lhs = self.resolve(lhs);
        let rhs = self.resolve(rhs);

        match (&lhs, &rhs) {
            _ if lhs == rhs => Ok(()),

            (Kind::Arrow(lhs_param, lhs_result), Kind::Arrow(rhs_param, rhs_result)) => {
                self.unify(lhs_param, rhs_param)?;
                self.unify(lhs_result, rhs_result)
            }

            (Kind::Meta(lhs), Kind::Meta(rhs)) => self.unify_metas(*lhs, *rhs),
            (Kind::Meta(var), kind) | (kind, Kind::Meta(var)) => self.bind(*var, kind),
            _ => Err(KindError::cannot_unify(lhs, rhs)),
        }
    }

    pub fn resolve(&mut self, kind: &Kind) -> Kind {
        match kind {
            Kind::Star => Kind::Star,
            Kind::Arrow(lhs, rhs) => Kind::make_arrow(self.resolve(lhs), self.resolve(rhs)),
            Kind::Meta(id) => {
                let var = self.var(*id);
                let (root, value) = self.table.inlined_probe_key_value(var);
                match value {
                    KindValue::Unknown => Kind::make_meta(self.meta_id(root)),
                    KindValue::Known(kind) => self.resolve(&kind),
                }
            }
        }
    }

    pub fn default_to_star(&mut self, kind: &Kind) -> Kind {
        match self.resolve(kind) {
            Kind::Star => Kind::Star,
            Kind::Arrow(lhs, rhs) => {
                Kind::make_arrow(self.default_to_star(&lhs), self.default_to_star(&rhs))
            }
            Kind::Meta(id) => {
                let var = self.var(id);
                let result = self
                    .table
                    .unify_var_value(var, KindValue::Known(Kind::Star));
                if let Err((lhs, rhs)) = result {
                    solveig_base::ice!(
                        "defaulting unresolved kind meta conflicted with `{}` and `{}`",
                        lhs.pretty(),
                        rhs.pretty()
                    );
                }
                Kind::Star
            }
        }
    }

    fn bind(&mut self, var: KindMetaId, kind: &Kind) -> Result<(), KindError> {
        let kind = self.resolve(kind);
        if kind == Kind::Meta(var) {
            return Ok(());
        }
        if kind_contains_meta(&kind, var) {
            return Err(KindError::occurs_check_failed(var, kind));
        }

        let var = self.var(var);
        self.table
            .unify_var_value(var, KindValue::Known(kind))
            .map_err(|(lhs, rhs)| KindError::cannot_unify(lhs, rhs))
    }

    fn unify_metas(&mut self, lhs: KindMetaId, rhs: KindMetaId) -> Result<(), KindError> {
        let lhs = self.var(lhs);
        let rhs = self.var(rhs);
        self.table
            .unify_var_var(lhs, rhs)
            .map_err(|(lhs, rhs)| KindError::cannot_unify(lhs, rhs))
    }

    fn var(&mut self, id: KindMetaId) -> KindVar {
        if let Some(var) = self.meta_vars.get(&id) {
            return *var;
        }

        self.insert_meta(id)
    }

    fn insert_meta(&mut self, id: KindMetaId) -> KindVar {
        let var = self.table.new_key(KindValue::Unknown);
        self.var_metas.push(id);
        self.meta_vars.insert(id, var);
        var
    }

    fn meta_id(&self, var: KindVar) -> KindMetaId {
        let index = var.index() as usize;
        *self
            .var_metas
            .get(index)
            .expect("ICE: kind unifier root missing public meta id")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KindVar(u32);

impl UnifyKey for KindVar {
    type Value = KindValue;

    fn index(&self) -> u32 {
        self.0
    }

    fn from_index(index: u32) -> Self {
        Self(index)
    }

    fn tag() -> &'static str {
        "Kind"
    }
}

#[derive(Clone, Debug)]
enum KindValue {
    Unknown,
    Known(Kind),
}

impl UnifyValue for KindValue {
    type Error = (Kind, Kind);

    fn unify_values(lhs: &Self, rhs: &Self) -> Result<Self, Self::Error> {
        match (lhs, rhs) {
            (Self::Unknown, value) | (value, Self::Unknown) => Ok(value.clone()),
            (Self::Known(lhs), Self::Known(rhs)) if lhs == rhs => Ok(Self::Known(lhs.clone())),
            (Self::Known(lhs), Self::Known(rhs)) => Err((lhs.clone(), rhs.clone())),
        }
    }
}

fn kind_contains_meta(kind: &Kind, needle: KindMetaId) -> bool {
    // `ena` cannot inspect recursive `Kind` syntax stored inside a root value,
    // so the shared unifier owns this single structural self-reference guard.
    match kind {
        Kind::Star => false,
        Kind::Meta(id) => *id == needle,
        Kind::Arrow(lhs, rhs) => kind_contains_meta(lhs, needle) || kind_contains_meta(rhs, needle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta_id(kind: &Kind) -> KindMetaId {
        match kind {
            Kind::Meta(id) => *id,
            other => panic!("expected kind meta, got {other:?}"),
        }
    }

    #[test]
    fn kind_unifier_resolves_and_defaults_arrow_components() {
        let mut unifier = KindUnifier::new();
        let fun = unifier.fresh();
        let arg = unifier.fresh();
        let result = unifier.fresh();

        unifier
            .unify(&fun, &Kind::make_arrow(arg.clone(), result))
            .expect("arrow-shaped kind should unify");
        unifier
            .unify(&arg, &Kind::Star)
            .expect("argument kind should unify with star");

        assert_eq!(
            unifier.default_to_star(&fun),
            Kind::make_arrow(Kind::Star, Kind::Star)
        );
    }

    #[test]
    fn kind_unifier_reports_recursive_kind_with_meta_id() {
        let mut unifier = KindUnifier::new();
        let meta = unifier.fresh();
        let meta_id = meta_id(&meta);
        let recursive = Kind::make_arrow(Kind::Star, meta.clone());

        let error = unifier
            .unify(&meta, &recursive)
            .expect_err("recursive kind should fail the occurs check");

        match error {
            KindError::OccursCheckFailed { var, in_kind, .. } => {
                assert_eq!(var, meta_id);
                assert_eq!(in_kind, recursive);
            }
            other => panic!("expected occurs-check error, got {other:?}"),
        }
    }

    #[test]
    fn kind_unifier_handles_sparse_external_meta_ids() {
        let mut unifier = KindUnifier::new();
        let seeded = Kind::make_meta(KindMetaId(u32::MAX));
        let fresh = unifier.fresh();

        unifier
            .unify(&seeded, &Kind::make_arrow(fresh.clone(), Kind::Star))
            .expect("sparse public kind meta should not allocate a sparse ena table");
        unifier
            .unify(&fresh, &Kind::Star)
            .expect("fresh kind meta should unify after sparse seeded meta");

        assert_eq!(
            unifier.resolve(&seeded),
            Kind::make_arrow(Kind::Star, Kind::Star)
        );
    }
}
