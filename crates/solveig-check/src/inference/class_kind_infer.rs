use std::collections::HashMap;

use solveig_ast::ast::{
    ClassDefinition, Kind, KindMetaId, Predicate, Scheme, TyVarId, TypeExpr, legacy_tyvar_id,
    tycon_display_name, tyvar_display_name,
};

use crate::signature_infer::KindError;

pub struct ClassKindInferencer {
    next_meta: u32,
    subst: HashMap<KindMetaId, Kind>,
    tycon_env: HashMap<String, Kind>,
    class_env: HashMap<String, Vec<Kind>>,
}

impl ClassKindInferencer {
    pub fn new(tycon_env: HashMap<String, Kind>, class_env: HashMap<String, Vec<Kind>>) -> Self {
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

    fn infer_type_kind(
        &mut self,
        ty: &TypeExpr,
        env: &mut HashMap<TyVarId, Kind>,
    ) -> Result<Kind, KindError> {
        match ty {
            TypeExpr::TyUnit => Ok(Kind::Star),

            TypeExpr::TyVar(id) => env
                .get(id)
                .cloned()
                .map(|k| self.apply_kind(&k))
                .ok_or_else(|| KindError::unbound_type_variable(tyvar_display_name(*id))),

            TypeExpr::TyExVar(_) => Ok(Kind::Star),

            TypeExpr::TyCon(id) => {
                let name = tycon_display_name(*id);
                self.tycon_env
                    .get(&name)
                    .cloned()
                    .map(|k| self.apply_kind(&k))
                    .ok_or_else(|| KindError::unbound_type_constructor(name))
            }

            TypeExpr::TyArr(lhs, rhs) => {
                let lhs_kind = self.infer_type_kind(lhs, env)?;
                let rhs_kind = self.infer_type_kind(rhs, env)?;
                self.unify(lhs_kind, Kind::Star)?;
                self.unify(rhs_kind, Kind::Star)?;
                Ok(Kind::Star)
            }

            TypeExpr::TyApp(lhs, rhs) => {
                let lhs_kind = self.infer_type_kind(lhs, env)?;
                let rhs_kind = self.infer_type_kind(rhs, env)?;
                let result_kind = self.fresh_meta();

                self.unify(lhs_kind, Kind::make_arrow(rhs_kind, result_kind.clone()))?;
                Ok(self.apply_kind(&result_kind))
            }

            TypeExpr::TyForAll(var_name, _old_kind, body) => {
                let fresh = self.fresh_meta();
                let old = env.insert(*var_name, fresh.clone());

                let body_kind = self.infer_type_kind(body, env)?;
                self.unify(body_kind, Kind::Star)?;

                match old {
                    Some(prev) => {
                        env.insert(*var_name, prev);
                    }
                    None => {
                        env.remove(var_name);
                    }
                }

                Ok(Kind::Star)
            }

            #[allow(unreachable_patterns)]
            TypeExpr::Qualified(preds, body) => {
                for pred in preds {
                    self.check_predicate(pred, env)?;
                }

                let body_kind = self.infer_type_kind(body, env)?;
                self.unify(body_kind, Kind::Star)?;
                Ok(Kind::Star)
            }
        }
    }

    fn zonk_type_with_env(&self, ty: &TypeExpr, env: &HashMap<TyVarId, Kind>) -> TypeExpr {
        match ty {
            TypeExpr::TyUnit => TypeExpr::TyUnit,
            TypeExpr::TyVar(id) => TypeExpr::TyVar(*id),
            TypeExpr::TyExVar(id) => TypeExpr::TyExVar(*id),
            TypeExpr::TyCon(id) => TypeExpr::TyCon(*id),
            TypeExpr::TyArr(lhs, rhs) => TypeExpr::make_arr(
                self.zonk_type_with_env(lhs, env),
                self.zonk_type_with_env(rhs, env),
            ),
            TypeExpr::TyApp(lhs, rhs) => TypeExpr::make_app(
                self.zonk_type_with_env(lhs, env),
                self.zonk_type_with_env(rhs, env),
            ),
            TypeExpr::TyForAll(name, _kind, body) => {
                let kind = env
                    .get(name)
                    .map(|k| self.zonk_kind_for_binder(k))
                    .unwrap_or(Kind::Star);

                TypeExpr::make_forall_id(*name, kind, self.zonk_type_with_env(body, env))
            }
            #[allow(unreachable_patterns)]
            TypeExpr::Qualified(preds, body) => TypeExpr::make_qualified(
                preds
                    .iter()
                    .map(|p| self.zonk_predicate_with_env(p, env))
                    .collect(),
                self.zonk_type_with_env(body, env),
            ),
        }
    }

    fn zonk_predicate_with_env(&self, pred: &Predicate, env: &HashMap<TyVarId, Kind>) -> Predicate {
        Predicate {
            class_name: pred.class_name.clone(),
            args: pred
                .args
                .iter()
                .map(|arg| self.zonk_type_with_env(arg, env))
                .collect(),
        }
    }

    fn check_predicate(
        &mut self,
        pred: &Predicate,
        env: &mut HashMap<TyVarId, Kind>,
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

        for (arg, expected_kind) in pred.args.iter().zip(expected_kinds.iter()) {
            let actual_kind = self.infer_type_kind(arg, env)?;
            self.unify(actual_kind, expected_kind.clone())?;
        }

        Ok(self.zonk_predicate_with_env(pred, env))
    }

    fn infer_scheme(
        &mut self,
        scheme: &Scheme,
        outer_env: &HashMap<TyVarId, Kind>,
    ) -> Result<Scheme, KindError> {
        let mut env = outer_env.clone();

        for (name, _kind) in &scheme.quantifiers {
            env.insert(*name, self.fresh_meta());
        }

        for pred in &scheme.predicates {
            self.check_predicate(pred, &mut env)?;
        }

        let body_kind = self.infer_type_kind(&scheme.ty, &mut env)?;
        self.unify(body_kind, Kind::Star)?;

        let quantifiers = scheme
            .quantifiers
            .iter()
            .map(|(name, _)| {
                let kind = env
                    .get(name)
                    .map(|k| self.zonk_kind_for_binder(k))
                    .unwrap_or(Kind::Star);
                (*name, kind)
            })
            .collect::<Vec<_>>();

        let predicates = scheme
            .predicates
            .iter()
            .map(|p| self.zonk_predicate_with_env(p, &env))
            .collect::<Vec<_>>();

        let ty = self.zonk_type_with_env(&scheme.ty, &env);

        Ok(Scheme {
            quantifiers,
            predicates,
            ty,
        })
    }

    pub fn infer_class(
        mut self,
        class_def: &ClassDefinition,
    ) -> Result<ClassDefinition, KindError> {
        let mut env = HashMap::<TyVarId, Kind>::new();

        for (name, _kind) in &class_def.params {
            env.insert(legacy_tyvar_id(name), self.fresh_meta());
        }

        let methods = class_def
            .methods
            .iter()
            .map(|method| {
                let scheme = self.infer_scheme(&method.scheme, &env)?;
                Ok(solveig_ast::ast::ClassMethodDef {
                    name: method.name.clone(),
                    scheme,
                })
            })
            .collect::<Result<Vec<_>, KindError>>()?;

        let superclasses = class_def
            .superclasses
            .iter()
            .map(|pred| self.check_predicate(pred, &mut env))
            .collect::<Result<Vec<_>, KindError>>()?;

        let params = class_def
            .params
            .iter()
            .map(|(name, _)| {
                let kind = env
                    .get(&legacy_tyvar_id(name))
                    .map(|k| self.zonk_kind_for_binder(k))
                    .unwrap_or(Kind::Star);
                (name.clone(), kind)
            })
            .collect::<Vec<_>>();

        Ok(ClassDefinition {
            name: class_def.name.clone(),
            params,
            superclasses,
            methods,
        })
    }
}
