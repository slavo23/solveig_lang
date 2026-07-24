use std::collections::HashMap;

use solveig_ast::ast::{
    ClassDefinition, ClassId, Kind, KindMetaId, Predicate, Scheme, TyConId, TyVarId, TypeExpr,
    legacy_tyvar_id, tyvar_display_name,
};

use crate::signature_infer::KindError;

pub struct ClassKindInferencer {
    next_meta: u32,
    subst: HashMap<KindMetaId, Kind>,
    tycon_env: HashMap<TyConId, Kind>,
    class_env: HashMap<ClassId, Vec<Kind>>,
}

impl ClassKindInferencer {
    pub fn new(tycon_env: HashMap<TyConId, Kind>, class_env: HashMap<ClassId, Vec<Kind>>) -> Self {
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

            TypeExpr::TyCon(id) => self
                .tycon_env
                .get(id)
                .cloned()
                .map(|k| self.apply_kind(&k))
                .ok_or_else(|| KindError::unbound_type_constructor(tycon_id_display(*id))),

            TypeExpr::TyArr(lhs, rhs) => {
                let lhs_kind = self.infer_type_kind(lhs, env)?;
                let rhs_kind = self.infer_type_kind(rhs, env)?;
                self.unify(lhs_kind, Kind::Star)?;
                self.unify(rhs_kind, Kind::Star)?;
                Ok(Kind::Star)
            }

            TypeExpr::TyApp(lhs, rhs) => {
                if matches!(**lhs, TypeExpr::TyForAll(_, _, _))
                    || matches!(**rhs, TypeExpr::TyForAll(_, _, _))
                {
                    return Err(KindError::cannot_unify(
                        Kind::Star,
                        Kind::make_arrow(Kind::Star, Kind::Star),
                    ));
                }

                let lhs_kind = self.infer_type_kind(lhs, env)?;
                let rhs_kind = self.infer_type_kind(rhs, env)?;
                let result_kind = self.fresh_meta();

                self.unify(lhs_kind, Kind::make_arrow(rhs_kind, result_kind.clone()))?;
                Ok(self.apply_kind(&result_kind))
            }

            TypeExpr::TyForAll(binders, preds, body) => {
                let mut old_bindings = Vec::with_capacity(binders.len());
                for (name, _) in binders {
                    old_bindings.push((*name, env.insert(*name, self.fresh_meta())));
                }

                for pred in preds {
                    self.check_predicate(pred, env)?;
                }

                let body_kind = self.infer_type_kind(body, env)?;
                self.unify(body_kind, Kind::Star)?;

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
            TypeExpr::TyForAll(binders, preds, body) => TypeExpr::make_forall_group(
                binders
                    .iter()
                    .map(|(name, _)| {
                        let kind = env
                            .get(name)
                            .map(|k| self.zonk_kind_for_binder(k))
                            .unwrap_or(Kind::Star);
                        (*name, kind)
                    })
                    .collect(),
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
            class_id: pred.class_id,
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
                    id: method.id,
                    name: method.name.clone(),
                    short_name: method.short_name.clone(),
                    scheme,
                    default_body: method.default_body.clone(),
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
            id: class_def.id,
            name: class_def.name.clone(),
            short_name: class_def.short_name.clone(),
            params,
            superclasses,
            methods,
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
    use solveig_ast::ast::{ClassMethodDef, DefId, ModuleId, legacy_tyvar_id};
    use solveig_base::Symbol;

    use super::*;

    #[test]
    fn class_kind_inference_uses_id_keyed_type_and_class_envs() {
        let tycon_id = TyConId(51);
        let class_id = ClassId(52);
        let superclass_id = ClassId(53);
        let param = legacy_tyvar_id("a");
        let class_def = ClassDefinition {
            id: class_id,
            name: "sample.WidgetLike".to_string(),
            short_name: "WidgetLike".to_string(),
            params: vec![("a".to_string(), Kind::Star)],
            superclasses: vec![Predicate {
                class_id: superclass_id,
                args: vec![TypeExpr::TyVar(param)],
            }],
            methods: vec![ClassMethodDef {
                id: DefId {
                    module: ModuleId(7),
                    name: Symbol(1),
                },
                name: "sample.WidgetLike.method".to_string(),
                short_name: "method".to_string(),
                scheme: Scheme {
                    quantifiers: Vec::new(),
                    predicates: Vec::new(),
                    ty: TypeExpr::TyCon(tycon_id),
                },
                default_body: None,
            }],
        };

        let inferred = ClassKindInferencer::new(
            HashMap::from([(tycon_id, Kind::Star)]),
            HashMap::from([(superclass_id, vec![Kind::Star])]),
        )
        .infer_class(&class_def)
        .expect("ICE: ID-keyed class kind environment should resolve test IDs");

        assert_eq!(inferred.id, class_id);
        assert_eq!(inferred.superclasses[0].class_id, superclass_id);
        assert_eq!(inferred.methods[0].scheme.ty, TypeExpr::TyCon(tycon_id));
    }
}
