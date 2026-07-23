impl TypeChecker {
    fn free_type_vars_in_type(ty: &TypeExpr, out: &mut BTreeSet<String>) {
        match ty {
            TypeExpr::TyVar(name) => {
                out.insert(name.clone());
            }

            TypeExpr::TyArr(lhs, rhs) | TypeExpr::TyApp(lhs, rhs) => {
                Self::free_type_vars_in_type(lhs, out);
                Self::free_type_vars_in_type(rhs, out);
            }

            TypeExpr::TyForAll(name, _, body) => {
                Self::free_type_vars_in_type(body, out);
                out.remove(name);
            }

            TypeExpr::Qualified(preds, body) => {
                for pred in preds {
                    Self::free_type_vars_in_predicate(pred, out);
                }

                Self::free_type_vars_in_type(body, out);
            }

            TypeExpr::TyUnit | TypeExpr::TyCon(_) | TypeExpr::TyExVar(_) => {}
        }
    }
    fn free_type_vars_in_predicate(pred: &Predicate, out: &mut BTreeSet<String>) {
        for arg in &pred.args {
            Self::free_type_vars_in_type(arg, out);
        }
    }
    fn free_type_vars_in_instance(inst: &InstanceDefinition<Span>) -> Vec<String> {
        let mut out = BTreeSet::new();

        for pred in &inst.context {
            Self::free_type_vars_in_predicate(pred, &mut out);
        }

        Self::free_type_vars_in_predicate(&inst.head, &mut out);

        out.into_iter().collect()
    }
    fn instance_quantifiers(inst: &InstanceDefinition<Span>) -> Vec<(String, Kind)> {
        Self::free_type_vars_in_instance(inst)
            .into_iter()
            .map(|name| (name, Kind::Star))
            .collect()
    }
    fn instantiate_predicate_with_subst(
        pred: &Predicate,
        subst: &HashMap<String, TypeExpr>,
    ) -> Predicate {
        Predicate {
            class_name: pred.class_name.clone(),
            args: pred
                .args
                .iter()
                .map(|arg| {
                    subst
                        .iter()
                        .fold(arg.clone(), |acc, (name, ty)| acc.substitute(name, ty))
                })
                .collect(),
        }
    }
    fn instantiate_scheme(
        &mut self,
        ctx: Context,
        scheme: &Scheme,
    ) -> (Context, Vec<Predicate>, TypeExpr) {
        let mut ctx = ctx;
        let mut subst = HashMap::<String, TypeExpr>::new();

        for (name, kind) in &scheme.quantifiers {
            let fresh = self.fresh_evar_id();
            ctx = ctx.add_entry(ContextEntry::ExVar(fresh, kind.clone()));
            subst.insert(name.clone(), Self::evar_type(fresh));
        }

        let instantiated_preds = scheme
            .predicates
            .iter()
            .map(|pred| Self::instantiate_predicate_with_subst(pred, &subst))
            .collect::<Vec<_>>();

        let instantiated_ty = subst.iter().fold(scheme.ty.clone(), |acc, (name, ty)| {
            acc.substitute(name, ty)
        });

        (ctx, instantiated_preds, instantiated_ty)
    }
    fn add_wanteds(&mut self, preds: Vec<Predicate>, span: Span) {
        let givens = self
            .givens
            .iter()
            .map(|pred| self.zonk_predicate(pred))
            .collect::<Vec<_>>();

        let zonked = preds
            .into_iter()
            .map(|pred| Wanted {
                pred: self.zonk_predicate(&pred),
                span: span.clone(),
                givens: givens.clone(),
            })
            .collect::<Vec<_>>();

        self.wanteds.extend(zonked);
    }
    fn with_givens<R>(&mut self, preds: Vec<Predicate>, f: impl FnOnce(&mut Self) -> R) -> R {
        let old_len = self.givens.len();
        self.givens.extend(preds);
        let result = f(self);
        self.givens.truncate(old_len);
        result
    }
    fn substitute_predicate(pred: &Predicate, subst: &HashMap<String, TypeExpr>) -> Predicate {
        Predicate {
            class_name: pred.class_name.clone(),
            args: pred
                .args
                .iter()
                .map(|arg| {
                    subst
                        .iter()
                        .fold(arg.clone(), |acc, (name, ty)| acc.substitute(name, ty))
                })
                .collect(),
        }
    }
    fn substitute_scheme(scheme: &Scheme, subst: &HashMap<String, TypeExpr>) -> Scheme {
        Scheme {
            quantifiers: scheme.quantifiers.clone(),

            predicates: scheme
                .predicates
                .iter()
                .map(|pred| Self::substitute_predicate(pred, subst))
                .collect(),

            ty: subst.iter().fold(scheme.ty.clone(), |acc, (name, ty)| {
                acc.substitute(name, ty)
            }),
        }
    }
    fn predicate_same_shape(a: &Predicate, b: &Predicate) -> bool {
        a.class_name == b.class_name && a.args == b.args
    }
    fn superclass_predicates_for_given(&self, given: &Predicate) -> Vec<Predicate> {
        let given = self.zonk_predicate(given);

        let Some(class_def) = self
            .class_defs
            .iter()
            .find(|class_def| class_def.name == given.class_name)
        else {
            return Vec::new();
        };

        if class_def.params.len() != given.args.len() {
            return Vec::new();
        }

        let subst = class_def
            .params
            .iter()
            .zip(given.args.iter())
            .map(|((param_name, _), arg)| (param_name.clone(), arg.clone()))
            .collect::<HashMap<_, _>>();

        class_def
            .superclasses
            .iter()
            .map(|pred| Self::instantiate_predicate_with_subst(pred, &subst))
            .map(|pred| self.zonk_predicate(&pred))
            .collect()
    }
    fn predicate_entails(
        &self,
        given: &Predicate,
        wanted: &Predicate,
        seen: &mut HashSet<Predicate>,
    ) -> bool {
        let given = self.zonk_predicate(given);
        let wanted = self.zonk_predicate(wanted);

        if given == wanted {
            return true;
        }

        if !seen.insert(given.clone()) {
            return false;
        }

        for superclass_pred in self.superclass_predicates_for_given(&given) {
            if self.predicate_entails(&superclass_pred, &wanted, seen) {
                seen.remove(&given);
                return true;
            }
        }

        seen.remove(&given);
        false
    }
    fn specialize_superclasses_for_instance(
        class_def: &ClassDefinition,
        instance_head: &Predicate,
    ) -> Vec<Predicate> {
        let subst = class_def
            .params
            .iter()
            .zip(instance_head.args.iter())
            .map(|((param_name, _), instance_arg)| (param_name.clone(), instance_arg.clone()))
            .collect::<HashMap<_, _>>();

        class_def
            .superclasses
            .iter()
            .map(|pred| Self::instantiate_predicate_with_subst(pred, &subst))
            .collect()
    }
    fn specialize_class_method_for_instance(
        class_def: &ClassDefinition,
        method_scheme: &Scheme,
        instance_head: &Predicate,
        instance_context: &[Predicate],
        instance_quantifiers: &[(String, Kind)],
    ) -> Scheme {
        let mut subst = HashMap::<String, TypeExpr>::new();

        for ((param_name, _), instance_arg) in
            class_def.params.iter().zip(instance_head.args.iter())
        {
            subst.insert(param_name.clone(), instance_arg.clone());
        }

        let specialized = Self::substitute_scheme(method_scheme, &subst);

        let specialized_class_head = Predicate {
            class_name: class_def.name.clone(),
            args: class_def
                .params
                .iter()
                .map(|(name, _)| {
                    subst
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| TypeExpr::TyVar(name.clone()))
                })
                .collect(),
        };

        let mut predicates = specialized
            .predicates
            .into_iter()
            .filter(|pred| !Self::predicate_same_shape(pred, &specialized_class_head))
            .collect::<Vec<_>>();

        predicates.extend(instance_context.iter().cloned());

        let mut quantifiers = instance_quantifiers.to_vec();

        for (name, kind) in specialized.quantifiers {
            if !quantifiers.iter().any(|(existing, _)| existing == &name) {
                quantifiers.push((name, kind));
            }
        }

        Scheme {
            quantifiers,
            predicates,
            ty: specialized.ty,
        }
    }
    fn check_instance_overlaps(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        let instances = &self.instance_defs;

        for (i, inst_a) in instances.iter().enumerate() {
            for inst_b in instances.iter().skip(i + 1) {
                if inst_a.head.class_name != inst_b.head.class_name {
                    continue;
                }
                if inst_a.head.args.len() != inst_b.head.args.len() {
                    continue;
                }

                let mut subst = HashMap::new();
                let overlaps = inst_a
                    .head
                    .args
                    .iter()
                    .zip(inst_b.head.args.iter())
                    .all(|(a, b)| Self::match_instance_type(a, b, &mut subst));

                if overlaps {
                    errors.push(TypeError::OverlappingInstances {
                        span: self.get_file_span(0..0),
                        class_name: inst_a.head.class_name.clone(),
                        instance_type: inst_b
                            .head
                            .args
                            .iter()
                            .map(|a| a.pretty())
                            .collect::<Vec<_>>()
                            .join(" "),
                    });
                }
            }
        }

        errors
    }
    fn match_instance_type(
        pattern: &TypeExpr,
        target: &TypeExpr,
        subst: &mut HashMap<String, TypeExpr>,
    ) -> bool {
        match pattern {
            TypeExpr::TyVar(name) => match subst.get(name) {
                Some(existing) => existing == target,
                None => {
                    subst.insert(name.clone(), target.clone());
                    true
                }
            },

            TypeExpr::TyCon(a) => matches!(target, TypeExpr::TyCon(b) if a == b),
            TypeExpr::TyUnit => matches!(target, TypeExpr::TyUnit),

            TypeExpr::TyApp(f1, a1) => match target {
                TypeExpr::TyApp(f2, a2) => {
                    Self::match_instance_type(f1, f2, subst)
                        && Self::match_instance_type(a1, a2, subst)
                }
                _ => false,
            },

            TypeExpr::TyArr(a1, b1) => match target {
                TypeExpr::TyArr(a2, b2) => {
                    Self::match_instance_type(a1, a2, subst)
                        && Self::match_instance_type(b1, b2, subst)
                }
                _ => false,
            },

            TypeExpr::TyForAll(_, _, body) => Self::match_instance_type(body, target, subst),
            TypeExpr::Qualified(_, body) => Self::match_instance_type(body, target, subst),

            TypeExpr::TyExVar(name) => matches!(target, TypeExpr::TyExVar(other) if name == other),
        }
    }
    fn discharge_predicate_with_givens(&self, pred: &Predicate, givens: &[Predicate]) -> bool {
        self.discharge_predicate_with_givens_inner(pred, givens, &mut HashSet::new())
    }
    fn discharge_predicate_with_givens_inner(
        &self,
        pred: &Predicate,
        givens: &[Predicate],
        seen: &mut HashSet<Predicate>,
    ) -> bool {
        let pred = self.zonk_predicate(pred);

        if givens
            .iter()
            .any(|given| self.predicate_entails(given, &pred, &mut HashSet::new()))
        {
            return true;
        }

        if !seen.insert(pred.clone()) {
            return false;
        }

        for inst in &self.instance_defs {
            if inst.head.class_name != pred.class_name || inst.head.args.len() != pred.args.len() {
                continue;
            }

            let mut subst = HashMap::<String, TypeExpr>::new();

            let head_matches =
                inst.head
                    .args
                    .iter()
                    .zip(pred.args.iter())
                    .all(|(head_arg, wanted_arg)| {
                        let wanted_arg = self.zonk_type(wanted_arg);
                        Self::match_instance_type(head_arg, &wanted_arg, &mut subst)
                    });

            if !head_matches {
                continue;
            }

            let all_context_ok = inst.context.iter().all(|ctx_pred| {
                let instantiated = Self::instantiate_predicate_with_subst(ctx_pred, &subst);
                self.discharge_predicate_with_givens_inner(&instantiated, givens, seen)
            });

            if all_context_ok {
                seen.remove(&pred);
                return true;
            }
        }

        seen.remove(&pred);
        false
    }
    fn unresolved_wanteds(&self) -> Vec<Wanted> {
        self.wanteds
            .iter()
            .cloned()
            .filter(|wanted| !self.discharge_predicate_with_givens(&wanted.pred, &wanted.givens))
            .collect()
    }
    fn wanteds_error(&self, wanteds: Vec<Wanted>) -> TypeError {
        let span = wanteds
            .first()
            .map(|w| self.get_file_span(w.span.clone()))
            .unwrap_or_else(|| self.get_file_span(0..0));

        TypeError::UnsatisfiedConstraints {
            span,
            predicates: wanteds
                .into_iter()
                .map(|w| self.zonk_predicate(&w.pred).pretty())
                .collect(),
        }
    }
    fn collect_exvars_in_type(ty: &TypeExpr, acc: &mut Vec<EvarId>) {
        match ty {
            TypeExpr::TyExVar(id) => {
                if !acc.contains(id) {
                    acc.push(*id);
                }
            }
            TypeExpr::TyArr(lhs, rhs) | TypeExpr::TyApp(lhs, rhs) => {
                Self::collect_exvars_in_type(lhs, acc);
                Self::collect_exvars_in_type(rhs, acc);
            }
            TypeExpr::TyForAll(_, _, body) => {
                Self::collect_exvars_in_type(body, acc);
            }
            TypeExpr::Qualified(preds, body) => {
                for pred in preds {
                    for arg in &pred.args {
                        Self::collect_exvars_in_type(arg, acc);
                    }
                }
                Self::collect_exvars_in_type(body, acc);
            }
            TypeExpr::TyUnit | TypeExpr::TyVar(_) | TypeExpr::TyCon(_) => {}
        }
    }
    fn generalize_top_level_type(&mut self, ctx: &Context, ty: &TypeExpr) -> TypeExpr {
        let zonked = self.zonk_type(&ctx.apply(ty));

        let mut evars = Vec::new();
        Self::collect_exvars_in_type(&zonked, &mut evars);

        if evars.is_empty() {
            return zonked;
        }

        let mut result = zonked;
        let mut binders = Vec::new();

        for evar in evars {
            let (universal_id, universal) = self.fresh_universal("t");
            let kind = ctx.evar_kind_id(evar).unwrap_or(Kind::Star);

            result = result.substitute_evar(evar, &TypeExpr::make_var_id("t", universal_id));
            binders.push((universal, kind));
        }

        for (name, kind) in binders.into_iter().rev() {
            result = TypeExpr::make_forall(name, kind, result);
        }

        result
    }
}
