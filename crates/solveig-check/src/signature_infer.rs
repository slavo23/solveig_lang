use std::collections::{HashMap, HashSet};

use solveig_ast::ast::{Kind, KindMetaId, Predicate, Pretty, Scheme, TypeExpr};
use solveig_base::{Diagnostic, FileSpan, IntoDiagnostic, Label};

#[derive(Debug, Clone)]
pub struct SignatureKindInfo {
    pub kinded_signature: TypeExpr,
    pub quantified_var_kinds: Vec<(String, Kind)>,
    pub signature_kind: Kind,
    pub kind_subst: HashMap<KindMetaId, Kind>,
}

#[derive(Debug, Clone)]
pub struct SchemeKindInfo {
    pub scheme: Scheme,
    pub signature_kind: Kind,
    pub kind_subst: HashMap<KindMetaId, Kind>,
}

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
        var: String,
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

    pub fn occurs_check_failed(var: impl Into<String>, in_kind: Kind) -> Self {
        Self::OccursCheckFailed {
            span: None,
            var: var.into(),
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
                    format!("type variable `{}` is not in scope", name),
                );
                if let Some(span) = span {
                    diagnostic = diagnostic.with_label(Label::primary(
                        *span,
                        format!("`{}` is used here, but no type variable was bound", name),
                    ));
                }
                diagnostic
            }

            Self::UnboundTypeConstructor { span, name } => {
                let mut diagnostic = Diagnostic::error(
                    "KIND0002",
                    format!("type constructor `{}` is not in scope", name),
                );
                if let Some(span) = span {
                    diagnostic = diagnostic.with_label(Label::primary(
                        *span,
                        format!("`{}` is used here, but no type constructor was found", name),
                    ));
                }
                diagnostic
            }

            Self::OccursCheckFailed { span, var, in_kind } => {
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

pub struct SignatureKindInferencer {
    next_meta: u32,
    subst: HashMap<KindMetaId, Kind>,
    tycon_env: HashMap<String, Kind>,
    class_env: HashMap<String, Vec<Kind>>,
}

impl Default for SignatureKindInferencer {
    fn default() -> Self {
        Self::new()
    }
}

impl SignatureKindInferencer {
    pub fn new() -> Self {
        Self {
            next_meta: 0,
            subst: HashMap::new(),
            tycon_env: HashMap::new(),
            class_env: HashMap::new(),
        }
    }

    pub fn with_tycon_env(tycon_env: HashMap<String, Kind>) -> Self {
        Self {
            next_meta: 0,
            subst: HashMap::new(),
            tycon_env,
            class_env: HashMap::new(),
        }
    }

    pub fn with_envs(
        tycon_env: HashMap<String, Kind>,
        class_env: HashMap<String, Vec<Kind>>,
    ) -> Self {
        Self {
            next_meta: 0,
            subst: HashMap::new(),
            tycon_env,
            class_env,
        }
    }

    fn fresh_meta(&mut self) -> Kind {
        let id = KindMetaId(self.next_meta);
        self.next_meta += 1;
        Kind::make_meta(id)
    }

    fn apply_kind(&self, kind: &Kind) -> Kind {
        match kind {
            Kind::Star => Kind::Star,
            Kind::Meta(id) => {
                if let Some(sol) = self.subst.get(id) {
                    self.apply_kind(sol)
                } else {
                    Kind::Meta(*id)
                }
            }
            Kind::Arrow(lhs, rhs) => Kind::make_arrow(self.apply_kind(lhs), self.apply_kind(rhs)),
        }
    }

    fn occurs(&self, needle: KindMetaId, kind: &Kind) -> bool {
        match self.apply_kind(kind) {
            Kind::Star => false,
            Kind::Meta(id) => id == needle,
            Kind::Arrow(lhs, rhs) => self.occurs(needle, &lhs) || self.occurs(needle, &rhs),
        }
    }

    fn bind_meta(&mut self, var: KindMetaId, kind: Kind) -> Result<(), KindError> {
        let kind = self.apply_kind(&kind);
        if kind == Kind::Meta(var) {
            return Ok(());
        }
        if self.occurs(var, &kind) {
            return Err(KindError::occurs_check_failed(format!("?k{}", var.0), kind));
        }
        self.subst.insert(var, kind);
        Ok(())
    }

    fn unify(&mut self, lhs: Kind, rhs: Kind) -> Result<(), KindError> {
        let lhs = self.apply_kind(&lhs);
        let rhs = self.apply_kind(&rhs);

        match (lhs, rhs) {
            (Kind::Star, Kind::Star) => Ok(()),
            (Kind::Meta(left), Kind::Meta(right)) if left == right => Ok(()),
            (Kind::Meta(v), other) => self.bind_meta(v, other),
            (other, Kind::Meta(v)) => self.bind_meta(v, other),
            (Kind::Arrow(l1, r1), Kind::Arrow(l2, r2)) => {
                self.unify(*l1, *l2)?;
                self.unify(*r1, *r2)
            }
            (lhs, rhs) => Err(KindError::cannot_unify(lhs, rhs)),
        }
    }

    fn default_unsolved(kind: Kind) -> Kind {
        match kind {
            Kind::Meta(_) => Kind::Star,
            Kind::Star => Kind::Star,
            Kind::Arrow(lhs, rhs) => {
                Kind::make_arrow(Self::default_unsolved(*lhs), Self::default_unsolved(*rhs))
            }
        }
    }

    fn zonk_kind_for_binder(&self, kind: &Kind) -> Kind {
        Self::default_unsolved(self.apply_kind(kind))
    }

    fn infer_predicate(
        &mut self,
        pred: &Predicate,
        env: &mut HashMap<String, Kind>,
    ) -> Result<Predicate, KindError> {
        let expected_kinds = self
            .class_env
            .get(&pred.class_name)
            .cloned()
            .ok_or_else(|| KindError::unbound_type_constructor(pred.class_name.clone()))?;

        if expected_kinds.len() != pred.args.len() {
            return Err(KindError::cannot_unify(
                Kind::Star,
                Kind::make_arrow(Kind::Star, Kind::Star),
            ));
        }

        let mut rebuilt_args = Vec::with_capacity(pred.args.len());

        for (arg, expected_kind) in pred.args.iter().zip(expected_kinds.iter()) {
            let (arg_kind, arg_kinded) = self.infer_and_rebuild(arg, env)?;
            self.unify(arg_kind, expected_kind.clone())?;
            rebuilt_args.push(arg_kinded);
        }

        Ok(Predicate {
            class_name: pred.class_name.clone(),
            args: rebuilt_args,
        })
    }

    fn infer_and_rebuild(
        &mut self,
        ty: &TypeExpr,
        env: &mut HashMap<String, Kind>,
    ) -> Result<(Kind, TypeExpr), KindError> {
        match ty {
            TypeExpr::TyUnit => Ok((Kind::Star, TypeExpr::TyUnit)),

            TypeExpr::TyVar(name) => {
                let kind = env
                    .get(name)
                    .cloned()
                    .map(|k| self.apply_kind(&k))
                    .ok_or_else(|| KindError::unbound_type_variable(name.clone()))?;
                Ok((kind, TypeExpr::TyVar(name.clone())))
            }

            TypeExpr::TyExVar(id) => Ok((Kind::Star, TypeExpr::TyExVar(*id))),

            TypeExpr::TyCon(name) => {
                let kind = self
                    .tycon_env
                    .get(name)
                    .cloned()
                    .map(|k| self.apply_kind(&k))
                    .ok_or_else(|| KindError::unbound_type_constructor(name.clone()))?;
                Ok((kind, TypeExpr::TyCon(name.clone())))
            }

            TypeExpr::TyArr(lhs, rhs) => {
                let (lhs_kind, lhs_kinded) = self.infer_and_rebuild(lhs, env)?;
                let (rhs_kind, rhs_kinded) = self.infer_and_rebuild(rhs, env)?;
                self.unify(lhs_kind, Kind::Star)?;
                self.unify(rhs_kind, Kind::Star)?;
                Ok((
                    Kind::Star,
                    TypeExpr::TyArr(Box::new(lhs_kinded), Box::new(rhs_kinded)),
                ))
            }

            TypeExpr::TyApp(lhs, rhs) => {
                let (lhs_kind, lhs_kinded) = self.infer_and_rebuild(lhs, env)?;
                let (rhs_kind, rhs_kinded) = self.infer_and_rebuild(rhs, env)?;
                let result_kind = self.fresh_meta();
                self.unify(lhs_kind, Kind::make_arrow(rhs_kind, result_kind.clone()))?;
                Ok((
                    self.apply_kind(&result_kind),
                    TypeExpr::TyApp(Box::new(lhs_kinded), Box::new(rhs_kinded)),
                ))
            }

            TypeExpr::TyForAll(var_name, _old_kind, body) => {
                let fresh = self.fresh_meta();
                let old = env.insert(var_name.clone(), fresh.clone());
                let (body_kind, body_kinded) = self.infer_and_rebuild(body, env)?;
                self.unify(body_kind, Kind::Star)?;

                match old {
                    Some(prev) => {
                        env.insert(var_name.clone(), prev);
                    }
                    None => {
                        env.remove(var_name);
                    }
                }

                let solved_binder_kind = self.zonk_kind_for_binder(&fresh);
                Ok((
                    Kind::Star,
                    TypeExpr::TyForAll(var_name.clone(), solved_binder_kind, Box::new(body_kinded)),
                ))
            }

            TypeExpr::Qualified(preds, body) => {
                let rebuilt_preds = preds
                    .iter()
                    .map(|pred| self.infer_predicate(pred, env))
                    .collect::<Result<Vec<_>, _>>()?;

                let (body_kind, body_kinded) = self.infer_and_rebuild(body, env)?;
                self.unify(body_kind, Kind::Star)?;

                Ok((
                    Kind::Star,
                    TypeExpr::Qualified(rebuilt_preds, Box::new(body_kinded)),
                ))
            }
        }
    }

    fn collect_outer_foralls(ty: &TypeExpr, out: &mut Vec<(String, Kind)>) {
        if let TypeExpr::TyForAll(name, kind, body) = ty {
            out.push((name.clone(), kind.clone()));
            Self::collect_outer_foralls(body, out);
        }
    }

    fn collect_free_tyvars_in_type(
        ty: &TypeExpr,
        bound: &mut Vec<String>,
        seen: &mut HashSet<String>,
        out: &mut Vec<String>,
    ) {
        match ty {
            TypeExpr::TyUnit | TypeExpr::TyCon(_) | TypeExpr::TyExVar(_) => {}

            TypeExpr::TyVar(name) => {
                if !bound.contains(name) && seen.insert(name.clone()) {
                    out.push(name.clone());
                }
            }

            TypeExpr::TyArr(lhs, rhs) | TypeExpr::TyApp(lhs, rhs) => {
                Self::collect_free_tyvars_in_type(lhs, bound, seen, out);
                Self::collect_free_tyvars_in_type(rhs, bound, seen, out);
            }

            TypeExpr::TyForAll(name, _, body) => {
                bound.push(name.clone());
                Self::collect_free_tyvars_in_type(body, bound, seen, out);
                bound.pop();
            }

            TypeExpr::Qualified(preds, body) => {
                for pred in preds {
                    for arg in &pred.args {
                        Self::collect_free_tyvars_in_type(arg, bound, seen, out);
                    }
                }
                Self::collect_free_tyvars_in_type(body, bound, seen, out);
            }
        }
    }

    fn collect_implicit_quantifiers(&self, scheme: &Scheme) -> Vec<String> {
        let mut bound = scheme
            .quantifiers
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        let mut seen = bound.iter().cloned().collect::<HashSet<_>>();
        let mut out = Vec::new();

        for pred in &scheme.predicates {
            for arg in &pred.args {
                Self::collect_free_tyvars_in_type(arg, &mut bound, &mut seen, &mut out);
            }
        }

        Self::collect_free_tyvars_in_type(&scheme.ty, &mut bound, &mut seen, &mut out);
        out
    }

    pub fn infer_signature(mut self, ty: &TypeExpr) -> Result<SignatureKindInfo, KindError> {
        let mut env = HashMap::<String, Kind>::new();
        let (sig_kind, kinded_sig) = self.infer_and_rebuild(ty, &mut env)?;
        let sig_kind = self.apply_kind(&sig_kind);

        if sig_kind != Kind::Star {
            return Err(KindError::signature_must_have_kind_star(sig_kind));
        }

        let mut quants = Vec::new();
        Self::collect_outer_foralls(&kinded_sig, &mut quants);

        Ok(SignatureKindInfo {
            kinded_signature: kinded_sig,
            quantified_var_kinds: quants,
            signature_kind: Kind::Star,
            kind_subst: self.subst,
        })
    }

    pub fn infer_scheme(mut self, scheme: &Scheme) -> Result<SchemeKindInfo, KindError> {
        let implicit_quants = self.collect_implicit_quantifiers(scheme);

        let mut quantifier_names = Vec::new();
        let mut env = HashMap::<String, Kind>::new();

        for (name, _old_kind) in &scheme.quantifiers {
            let fresh = self.fresh_meta();
            env.insert(name.clone(), fresh);
            quantifier_names.push(name.clone());
        }

        for name in implicit_quants {
            let fresh = self.fresh_meta();
            env.insert(name.clone(), fresh);
            quantifier_names.push(name);
        }

        let rebuilt_preds = scheme
            .predicates
            .iter()
            .map(|pred| self.infer_predicate(pred, &mut env))
            .collect::<Result<Vec<_>, _>>()?;

        let (body_kind, rebuilt_ty) = self.infer_and_rebuild(&scheme.ty, &mut env)?;
        let body_kind = self.apply_kind(&body_kind);

        if body_kind != Kind::Star {
            return Err(KindError::signature_must_have_kind_star(body_kind));
        }

        let quantifiers = quantifier_names
            .into_iter()
            .map(|name| {
                let kind = env.get(&name).cloned().unwrap_or(Kind::Star);
                (name, self.zonk_kind_for_binder(&kind))
            })
            .collect::<Vec<_>>();

        Ok(SchemeKindInfo {
            scheme: Scheme {
                quantifiers,
                predicates: rebuilt_preds,
                ty: rebuilt_ty,
            },
            signature_kind: Kind::Star,
            kind_subst: self.subst,
        })
    }
}
