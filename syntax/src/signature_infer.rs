use std::collections::HashMap;

use core::ast::{Kind, TypeExpr};

#[derive(Debug, Clone)]
pub struct SignatureKindInfo {
    pub kinded_signature: TypeExpr,
    pub quantified_var_kinds: Vec<(String, Kind)>,
    pub signature_kind: Kind,
    pub kind_subst: HashMap<String, Kind>,
}

#[derive(Debug, Clone)]
pub enum KindError {
    UnboundTypeVariable(String),
    UnboundTypeConstructor(String),
    OccursCheckFailed { var: String, in_kind: Kind },
    CannotUnify { lhs: Kind, rhs: Kind },
    SignatureMustHaveKindStar { got: Kind },
}

pub struct SignatureKindInferencer {
    next_meta: usize,
    subst: HashMap<String, Kind>,
    tycon_env: HashMap<String, Kind>,
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
        }
    }

    pub fn with_tycon_env(tycon_env: HashMap<String, Kind>) -> Self {
        Self {
            next_meta: 0,
            subst: HashMap::new(),
            tycon_env,
        }
    }

    fn fresh_meta(&mut self) -> Kind {
        let name = format!("?k{}", self.next_meta);
        self.next_meta += 1;
        Kind::make_var(name)
    }

    fn apply_kind(&self, kind: &Kind) -> Kind {
        match kind {
            Kind::Star => Kind::Star,
            Kind::Var(v) => {
                if let Some(sol) = self.subst.get(v) {
                    self.apply_kind(sol)
                } else {
                    Kind::Var(v.clone())
                }
            }
            Kind::Arrow(lhs, rhs) => Kind::make_arrow(self.apply_kind(lhs), self.apply_kind(rhs)),
        }
    }

    fn occurs(&self, needle: &str, kind: &Kind) -> bool {
        match self.apply_kind(kind) {
            Kind::Star => false,
            Kind::Var(v) => v == needle,
            Kind::Arrow(lhs, rhs) => self.occurs(needle, &lhs) || self.occurs(needle, &rhs),
        }
    }

    fn bind_meta(&mut self, var: String, kind: Kind) -> Result<(), KindError> {
        let kind = self.apply_kind(&kind);
        if kind == Kind::Var(var.clone()) {
            return Ok(());
        }
        if self.occurs(&var, &kind) {
            return Err(KindError::OccursCheckFailed { var, in_kind: kind });
        }
        self.subst.insert(var, kind);
        Ok(())
    }

    fn unify(&mut self, lhs: Kind, rhs: Kind) -> Result<(), KindError> {
        let lhs = self.apply_kind(&lhs);
        let rhs = self.apply_kind(&rhs);

        match (lhs, rhs) {
            (Kind::Star, Kind::Star) => Ok(()),
            (Kind::Var(v), other) => self.bind_meta(v, other),
            (other, Kind::Var(v)) => self.bind_meta(v, other),
            (Kind::Arrow(l1, r1), Kind::Arrow(l2, r2)) => {
                self.unify(*l1, *l2)?;
                self.unify(*r1, *r2)
            }
            (lhs, rhs) => Err(KindError::CannotUnify { lhs, rhs }),
        }
    }

    fn default_unsolved(kind: Kind) -> Kind {
        match kind {
            Kind::Var(_) => Kind::Star,
            Kind::Star => Kind::Star,
            Kind::Arrow(lhs, rhs) => {
                Kind::make_arrow(Self::default_unsolved(*lhs), Self::default_unsolved(*rhs))
            }
        }
    }

    fn zonk_kind_for_binder(&self, kind: &Kind) -> Kind {
        Self::default_unsolved(self.apply_kind(kind))
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
                    .ok_or_else(|| KindError::UnboundTypeVariable(name.clone()))?;
                Ok((kind, TypeExpr::TyVar(name.clone())))
            }

            TypeExpr::TyExVar(name) => {
                let kind = env
                    .get(name)
                    .cloned()
                    .map(|k| self.apply_kind(&k))
                    .ok_or_else(|| KindError::UnboundTypeVariable(name.clone()))?;
                Ok((kind, TypeExpr::TyExVar(name.clone())))
            }

            TypeExpr::TyCon(name) => {
                let kind = self
                    .tycon_env
                    .get(name)
                    .cloned()
                    .map(|k| self.apply_kind(&k))
                    .ok_or_else(|| KindError::UnboundTypeConstructor(name.clone()))?;
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
        }
    }

    fn collect_outer_foralls(ty: &TypeExpr, out: &mut Vec<(String, Kind)>) {
        if let TypeExpr::TyForAll(name, kind, body) = ty {
            out.push((name.clone(), kind.clone()));
            Self::collect_outer_foralls(body, out);
        }
    }

    pub fn infer_signature(mut self, ty: &TypeExpr) -> Result<SignatureKindInfo, KindError> {
        let mut env = HashMap::<String, Kind>::new();
        let (sig_kind, kinded_sig) = self.infer_and_rebuild(ty, &mut env)?;
        let sig_kind = self.apply_kind(&sig_kind);

        if sig_kind != Kind::Star {
            return Err(KindError::SignatureMustHaveKindStar { got: sig_kind });
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
}
