//! Kind inference for typeclass declarations and superclass predicates.
//! This pass shares kind unification machinery with signatures and data definitions.

use solveig_base::FxHashMap;

use solveig_ast::ast::{
    ClassDefinition, ClassId, Kind, Predicate, Scheme, Ty, TyConId, TyVarId, tyvar_display_name,
};

use crate::kind_unify::{KindError, KindUnifier};

pub struct ClassKindInferencer {
    unifier: KindUnifier,
    tycon_env: FxHashMap<TyConId, Kind>,
    class_env: FxHashMap<ClassId, Vec<Kind>>,
}

impl ClassKindInferencer {
    pub fn new(
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

    fn infer_type_kind(
        &mut self,
        ty: &Ty,
        env: &mut FxHashMap<TyVarId, Kind>,
    ) -> Result<Kind, KindError> {
        match ty {
            Ty::TyUnit | Ty::TyExVar(_) => Ok(Kind::Star),

            Ty::TyVar(id) => env
                .get(id)
                .cloned()
                .map(|k| self.unifier.resolve(&k))
                .ok_or_else(|| KindError::unbound_type_variable(tyvar_display_name(*id))),

            Ty::TyCon(id) => self
                .tycon_env
                .get(id)
                .cloned()
                .map(|k| self.unifier.resolve(&k))
                .ok_or_else(|| KindError::unbound_type_constructor(tycon_id_display(*id))),

            Ty::TyArr(lhs, rhs) => {
                let lhs_kind = self.infer_type_kind(lhs, env)?;
                let rhs_kind = self.infer_type_kind(rhs, env)?;
                self.unifier.unify(&lhs_kind, &Kind::Star)?;
                self.unifier.unify(&rhs_kind, &Kind::Star)?;
                Ok(Kind::Star)
            }

            Ty::TyApp(lhs, rhs) => {
                if matches!(**lhs, Ty::TyForAll(_, _, _)) || matches!(**rhs, Ty::TyForAll(_, _, _))
                {
                    return Err(KindError::cannot_unify(
                        Kind::Star,
                        Kind::make_arrow(Kind::Star, Kind::Star),
                    ));
                }

                let lhs_kind = self.infer_type_kind(lhs, env)?;
                let rhs_kind = self.infer_type_kind(rhs, env)?;
                let result_kind = self.unifier.fresh();

                self.unifier
                    .unify(&lhs_kind, &Kind::make_arrow(rhs_kind, result_kind.clone()))?;
                Ok(self.unifier.resolve(&result_kind))
            }

            Ty::TyForAll(binders, preds, body) => {
                let mut old_bindings = Vec::with_capacity(binders.len());
                for (name, _) in binders {
                    old_bindings.push((*name, env.insert(*name, self.unifier.fresh())));
                }

                for pred in preds {
                    self.check_predicate(pred, env)?;
                }

                let body_kind = self.infer_type_kind(body, env)?;
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

                Ok(Kind::Star)
            }
        }
    }

    fn zonk_type_with_env(&mut self, ty: &Ty, env: &FxHashMap<TyVarId, Kind>) -> Ty {
        match ty {
            Ty::TyUnit => Ty::TyUnit,
            Ty::TyVar(id) => Ty::TyVar(*id),
            Ty::TyExVar(id) => Ty::TyExVar(*id),
            Ty::TyCon(id) => Ty::TyCon(*id),
            Ty::TyArr(lhs, rhs) => Ty::make_arr(
                self.zonk_type_with_env(lhs, env),
                self.zonk_type_with_env(rhs, env),
            ),
            Ty::TyApp(lhs, rhs) => Ty::make_app(
                self.zonk_type_with_env(lhs, env),
                self.zonk_type_with_env(rhs, env),
            ),
            Ty::TyForAll(binders, preds, body) => Ty::make_forall_group(
                binders
                    .iter()
                    .map(|(name, _)| {
                        let kind = env
                            .get(name)
                            .map_or(Kind::Star, |k| self.zonk_kind_for_binder(k));
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

    fn zonk_predicate_with_env(
        &mut self,
        pred: &Predicate,
        env: &FxHashMap<TyVarId, Kind>,
    ) -> Predicate {
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

        for (arg, expected_kind) in pred.args.iter().zip(expected_kinds.iter()) {
            let actual_kind = self.infer_type_kind(arg, env)?;
            self.unifier.unify(&actual_kind, expected_kind)?;
        }

        Ok(self.zonk_predicate_with_env(pred, env))
    }

    fn infer_scheme(
        &mut self,
        scheme: &Scheme,
        outer_env: &FxHashMap<TyVarId, Kind>,
    ) -> Result<Scheme, KindError> {
        let mut env = outer_env.clone();

        for (name, _kind) in &scheme.quantifiers {
            env.insert(*name, self.unifier.fresh());
        }

        for pred in &scheme.predicates {
            self.check_predicate(pred, &mut env)?;
        }

        let body_kind = self.infer_type_kind(&scheme.ty, &mut env)?;
        self.unifier.unify(&body_kind, &Kind::Star)?;

        let quantifiers = scheme
            .quantifiers
            .iter()
            .map(|(name, _)| {
                let kind = env
                    .get(name)
                    .map_or(Kind::Star, |k| self.zonk_kind_for_binder(k));
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
        let mut env = FxHashMap::<TyVarId, Kind>::default();

        for (id, _kind) in &class_def.params {
            env.insert(*id, self.unifier.fresh());
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
            .map(|(id, _)| {
                let kind = env
                    .get(id)
                    .map_or(Kind::Star, |k| self.zonk_kind_for_binder(k));
                (*id, kind)
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
    use solveig_ast::ast::{ClassMethodDef, DefId, ModuleId};
    use solveig_base::Symbol;

    use super::*;

    #[test]
    fn class_kind_inference_uses_id_keyed_type_and_class_envs() {
        let tycon_id = TyConId(51);
        let class_id = ClassId(52);
        let superclass_id = ClassId(53);
        let param = TyVarId(0);
        let class_def = ClassDefinition {
            id: class_id,
            name: "sample.WidgetLike".to_string(),
            short_name: "WidgetLike".to_string(),
            params: vec![(param, Kind::Star)],
            superclasses: vec![Predicate {
                class_id: superclass_id,
                args: vec![Ty::TyVar(param)],
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
                    ty: Ty::TyCon(tycon_id),
                },
                default_body: None,
            }],
        };

        let inferred = ClassKindInferencer::new(
            FxHashMap::from_iter([(tycon_id, Kind::Star)]),
            FxHashMap::from_iter([(superclass_id, vec![Kind::Star])]),
        )
        .infer_class(&class_def)
        .expect("ICE: ID-keyed class kind environment should resolve test IDs");

        assert_eq!(inferred.id, class_id);
        assert_eq!(inferred.superclasses[0].class_id, superclass_id);
        assert_eq!(inferred.methods[0].scheme.ty, Ty::TyCon(tycon_id));
    }
}
