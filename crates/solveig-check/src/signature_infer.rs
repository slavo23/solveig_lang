//! Kind inference for user-written type signatures and predicates.
//! Resolver and checker stages use this pass before term type checking.

use solveig_base::{FxHashMap, FxHashSet};

use solveig_ast::ast::{
    ClassId, Kind, Predicate, Scheme, Ty, TyConId, TyVarId, tyvar_display_name,
};

pub use crate::kind_unify::KindError;
use crate::kind_unify::KindUnifier;

#[derive(Debug, Clone)]
pub struct SignatureKindInfo {
    pub kinded_signature: Ty,
    pub quantified_var_kinds: Vec<(TyVarId, Kind)>,
    pub signature_kind: Kind,
}

#[derive(Debug, Clone)]
pub struct SchemeKindInfo {
    pub scheme: Scheme,
    pub signature_kind: Kind,
}

pub struct SignatureKindInferencer {
    unifier: KindUnifier,
    tycon_env: FxHashMap<TyConId, Kind>,
    class_env: FxHashMap<ClassId, Vec<Kind>>,
}

impl Default for SignatureKindInferencer {
    fn default() -> Self {
        Self::new()
    }
}

impl SignatureKindInferencer {
    pub fn new() -> Self {
        Self {
            unifier: KindUnifier::new(),
            tycon_env: FxHashMap::default(),
            class_env: FxHashMap::default(),
        }
    }

    pub fn with_tycon_env(tycon_env: FxHashMap<TyConId, Kind>) -> Self {
        Self {
            unifier: KindUnifier::new(),
            tycon_env,
            class_env: FxHashMap::default(),
        }
    }

    pub fn with_envs(
        tycon_env: FxHashMap<TyConId, Kind>,
        class_env: FxHashMap<ClassId, Vec<Kind>>,
    ) -> Self {
        Self {
            unifier: KindUnifier::new(),
            tycon_env,
            class_env,
        }
    }

    fn zonk_kind_for_binder(&mut self, kind: &Kind) -> Kind {
        self.unifier.default_to_star(kind)
    }

    fn infer_predicate(
        &mut self,
        pred: &Predicate,
        env: &mut FxHashMap<TyVarId, Kind>,
    ) -> Result<Predicate, KindError> {
        let expected_kinds =
            self.class_env.get(&pred.class_id).cloned().ok_or_else(|| {
                KindError::unbound_type_constructor(class_id_display(pred.class_id))
            })?;

        if expected_kinds.len() != pred.args.len() {
            return Err(KindError::cannot_unify(
                Kind::Star,
                Kind::make_arrow(Kind::Star, Kind::Star),
            ));
        }

        let mut rebuilt_args = Vec::with_capacity(pred.args.len());

        for (arg, expected_kind) in pred.args.iter().zip(expected_kinds.iter()) {
            let (arg_kind, arg_kinded) = self.infer_and_rebuild(arg, env)?;
            self.unifier.unify(&arg_kind, expected_kind)?;
            rebuilt_args.push(arg_kinded);
        }

        Ok(Predicate {
            class_id: pred.class_id,
            args: rebuilt_args,
        })
    }

    fn infer_and_rebuild(
        &mut self,
        ty: &Ty,
        env: &mut FxHashMap<TyVarId, Kind>,
    ) -> Result<(Kind, Ty), KindError> {
        match ty {
            Ty::TyUnit => Ok((Kind::Star, Ty::TyUnit)),

            Ty::TyVar(id) => {
                let kind = env
                    .get(id)
                    .cloned()
                    .map(|k| self.unifier.resolve(&k))
                    .ok_or_else(|| KindError::unbound_type_variable(tyvar_display_name(*id)))?;
                Ok((kind, Ty::TyVar(*id)))
            }

            Ty::TyExVar(id) => Ok((Kind::Star, Ty::TyExVar(*id))),

            Ty::TyCon(id) => {
                let kind = self
                    .tycon_env
                    .get(id)
                    .cloned()
                    .map(|k| self.unifier.resolve(&k))
                    .ok_or_else(|| KindError::unbound_type_constructor(tycon_id_display(*id)))?;
                Ok((kind, Ty::TyCon(*id)))
            }

            Ty::TyArr(lhs, rhs) => {
                let (lhs_kind, lhs_kinded) = self.infer_and_rebuild(lhs, env)?;
                let (rhs_kind, rhs_kinded) = self.infer_and_rebuild(rhs, env)?;
                self.unifier.unify(&lhs_kind, &Kind::Star)?;
                self.unifier.unify(&rhs_kind, &Kind::Star)?;
                Ok((
                    Kind::Star,
                    Ty::TyArr(Box::new(lhs_kinded), Box::new(rhs_kinded)),
                ))
            }

            Ty::TyApp(lhs, rhs) => {
                if matches!(**lhs, Ty::TyForAll(_, _, _)) || matches!(**rhs, Ty::TyForAll(_, _, _))
                {
                    return Err(KindError::cannot_unify(
                        Kind::Star,
                        Kind::make_arrow(Kind::Star, Kind::Star),
                    ));
                }

                let (lhs_kind, lhs_kinded) = self.infer_and_rebuild(lhs, env)?;
                let (rhs_kind, rhs_kinded) = self.infer_and_rebuild(rhs, env)?;
                let result_kind = self.unifier.fresh();
                self.unifier
                    .unify(&lhs_kind, &Kind::make_arrow(rhs_kind, result_kind.clone()))?;
                Ok((
                    self.unifier.resolve(&result_kind),
                    Ty::TyApp(Box::new(lhs_kinded), Box::new(rhs_kinded)),
                ))
            }

            Ty::TyForAll(binders, preds, body) => {
                let mut binder_metas = Vec::with_capacity(binders.len());
                let mut old_bindings = Vec::with_capacity(binders.len());
                for (name, _) in binders {
                    let fresh = self.unifier.fresh();
                    old_bindings.push((*name, env.insert(*name, fresh.clone())));
                    binder_metas.push((*name, fresh));
                }

                let rebuilt_preds = preds
                    .iter()
                    .map(|pred| self.infer_predicate(pred, env))
                    .collect::<Result<Vec<_>, _>>()?;

                let (body_kind, body_kinded) = self.infer_and_rebuild(body, env)?;
                self.unifier.unify(&body_kind, &Kind::Star)?;

                for (name, old) in old_bindings.into_iter().rev() {
                    match old {
                        Some(prev) => {
                            env.insert(name, prev);
                        }
                        None => {
                            env.remove(&name);
                        }
                    }
                }

                Ok((
                    Kind::Star,
                    Ty::make_forall_group(
                        binder_metas
                            .into_iter()
                            .map(|(name, kind)| (name, self.zonk_kind_for_binder(&kind)))
                            .collect(),
                        rebuilt_preds,
                        body_kinded,
                    ),
                ))
            }
        }
    }

    fn collect_outer_foralls(ty: &Ty, out: &mut Vec<(TyVarId, Kind)>) {
        if let Ty::TyForAll(binders, _, body) = ty {
            out.extend(binders.iter().cloned());
            Self::collect_outer_foralls(body, out);
        }
    }

    fn collect_free_tyvars_in_type(
        ty: &Ty,
        bound: &mut Vec<TyVarId>,
        seen: &mut FxHashSet<TyVarId>,
        out: &mut Vec<TyVarId>,
    ) {
        match ty {
            Ty::TyUnit | Ty::TyCon(_) | Ty::TyExVar(_) => {}

            Ty::TyVar(id) => {
                if !bound.contains(id) && seen.insert(*id) {
                    out.push(*id);
                }
            }

            Ty::TyArr(lhs, rhs) | Ty::TyApp(lhs, rhs) => {
                Self::collect_free_tyvars_in_type(lhs, bound, seen, out);
                Self::collect_free_tyvars_in_type(rhs, bound, seen, out);
            }

            Ty::TyForAll(binders, preds, body) => {
                for (name, _) in binders {
                    bound.push(*name);
                }
                for pred in preds {
                    for arg in &pred.args {
                        Self::collect_free_tyvars_in_type(arg, bound, seen, out);
                    }
                }
                Self::collect_free_tyvars_in_type(body, bound, seen, out);
                for _ in binders {
                    bound.pop();
                }
            }
        }
    }

    fn collect_implicit_quantifiers(&self, scheme: &Scheme) -> Vec<TyVarId> {
        let mut bound = scheme
            .quantifiers
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>();
        let mut seen = bound.iter().copied().collect::<FxHashSet<_>>();
        let mut out = Vec::new();

        for pred in &scheme.predicates {
            for arg in &pred.args {
                Self::collect_free_tyvars_in_type(arg, &mut bound, &mut seen, &mut out);
            }
        }

        Self::collect_free_tyvars_in_type(&scheme.ty, &mut bound, &mut seen, &mut out);
        out
    }

    pub fn infer_signature(mut self, ty: &Ty) -> Result<SignatureKindInfo, KindError> {
        let mut env = FxHashMap::<TyVarId, Kind>::default();
        let (sig_kind, kinded_sig) = self.infer_and_rebuild(ty, &mut env)?;
        let sig_kind = self.unifier.resolve(&sig_kind);

        if sig_kind != Kind::Star {
            return Err(KindError::signature_must_have_kind_star(sig_kind));
        }

        let mut quants = Vec::new();
        Self::collect_outer_foralls(&kinded_sig, &mut quants);

        Ok(SignatureKindInfo {
            kinded_signature: kinded_sig,
            quantified_var_kinds: quants,
            signature_kind: Kind::Star,
        })
    }

    pub fn infer_scheme(mut self, scheme: &Scheme) -> Result<SchemeKindInfo, KindError> {
        let implicit_quants = self.collect_implicit_quantifiers(scheme);

        let mut quantifier_names = Vec::new();
        let mut env = FxHashMap::<TyVarId, Kind>::default();

        for (name, _old_kind) in &scheme.quantifiers {
            let fresh = self.unifier.fresh();
            env.insert(*name, fresh);
            quantifier_names.push(*name);
        }

        for name in implicit_quants {
            let fresh = self.unifier.fresh();
            env.insert(name, fresh);
            quantifier_names.push(name);
        }

        let rebuilt_preds = scheme
            .predicates
            .iter()
            .map(|pred| self.infer_predicate(pred, &mut env))
            .collect::<Result<Vec<_>, _>>()?;

        let (body_kind, rebuilt_ty) = self.infer_and_rebuild(&scheme.ty, &mut env)?;
        let body_kind = self.unifier.resolve(&body_kind);

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
        })
    }
}

fn tycon_id_display(id: TyConId) -> String {
    format!("T{}", id.0)
}

fn class_id_display(id: ClassId) -> String {
    format!("K{}", id.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_kind_inference_uses_id_keyed_type_and_class_envs() {
        let tycon_id = TyConId(41);
        let class_id = ClassId(42);
        let scheme = Scheme {
            quantifiers: Vec::new(),
            predicates: vec![Predicate {
                class_id,
                args: vec![Ty::TyCon(tycon_id)],
            }],
            ty: Ty::TyCon(tycon_id),
        };

        let info = SignatureKindInferencer::with_envs(
            FxHashMap::from_iter([(tycon_id, Kind::Star)]),
            FxHashMap::from_iter([(class_id, vec![Kind::Star])]),
        )
        .infer_scheme(&scheme)
        .expect("ICE: ID-keyed signature kind environment should resolve test IDs");

        assert_eq!(info.scheme.ty, Ty::TyCon(tycon_id));
        assert_eq!(info.scheme.predicates[0].class_id, class_id);
    }
}
