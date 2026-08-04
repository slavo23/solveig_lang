#[derive(Debug)]
struct GeneralizedInferred {
    ty: Ty,
    subst: FxHashMap<EvarId, Ty>,
    retained: Vec<GivenEvidence>,
}

impl TypeChecker<'_> {
    fn free_type_vars_in_type(ty: &Ty, out: &mut BTreeSet<TyVarId>) {
        match ty {
            Ty::TyVar(id) => {
                out.insert(*id);
            }

            Ty::TyArr(lhs, rhs) | Ty::TyApp(lhs, rhs) => {
                Self::free_type_vars_in_type(lhs, out);
                Self::free_type_vars_in_type(rhs, out);
            }

            Ty::TyForAll(binders, preds, body) => {
                for pred in preds {
                    Self::free_type_vars_in_predicate(pred, out);
                }
                Self::free_type_vars_in_type(body, out);
                for (name, _) in binders {
                    out.remove(name);
                }
            }

            Ty::TyUnit | Ty::TyCon(_) | Ty::TyExVar(_) => {}
        }
    }
    fn free_type_vars_in_predicate(pred: &Predicate, out: &mut BTreeSet<TyVarId>) {
        for arg in &pred.args {
            Self::free_type_vars_in_type(arg, out);
        }
    }
    fn free_type_vars_in_instance(inst: &InstanceDefinition<Span>) -> Vec<TyVarId> {
        let mut out = BTreeSet::new();

        for pred in &inst.context {
            Self::free_type_vars_in_predicate(pred, &mut out);
        }

        Self::free_type_vars_in_predicate(&inst.head, &mut out);

        out.into_iter().collect()
    }
    fn instance_quantifiers(inst: &InstanceDefinition<Span>) -> Vec<(TyVarId, Kind)> {
        Self::free_type_vars_in_instance(inst)
            .into_iter()
            .map(|name| (name, Kind::Star))
            .collect()
    }
    fn instantiate_predicate_with_subst(
        pred: &Predicate,
        subst: &FxHashMap<TyVarId, Ty>,
    ) -> Predicate {
        Predicate {
            class_id: pred.class_id,
            args: pred
                .args
                .iter()
                .map(|arg| {
                    subst
                        .iter()
                        .fold(arg.clone(), |acc, (name, ty)| acc.substitute(*name, ty))
                })
                .collect(),
        }
    }
    fn instantiate_scheme(
        &mut self,
        ctx: Context,
        scheme: &Scheme,
    ) -> (Context, Vec<Predicate>, Ty) {
        let mut ctx = ctx;
        let mut subst = FxHashMap::<TyVarId, Ty>::default();

        for (name, kind) in &scheme.quantifiers {
            let fresh = self.fresh_evar_id();
            ctx = ctx.add_entry(ContextEntry::ExVar(fresh, kind.clone()));
            subst.insert(*name, Self::evar_type(fresh));
        }

        let instantiated_preds = scheme
            .predicates
            .iter()
            .map(|pred| Self::instantiate_predicate_with_subst(pred, &subst))
            .collect::<Vec<_>>();

        let instantiated_ty = subst.iter().fold(scheme.ty.clone(), |acc, (name, ty)| {
            acc.substitute(*name, ty)
        });

        (ctx, instantiated_preds, instantiated_ty)
    }
    fn add_wanteds(&mut self, preds: Vec<Predicate>, span: Span) -> Vec<EvVarId> {
        let givens = self
            .givens
            .iter()
            .map(|given| GivenEvidence {
                ev: given.ev,
                pred: self.zonk_predicate(&given.pred),
            })
            .collect::<Vec<_>>();

        let mut evvars = Vec::new();
        let mut zonked = Vec::new();

        for pred in preds {
            let ev = self.fresh_evvar_id();
            evvars.push(ev);
            zonked.push(Wanted {
                ev,
                pred: self.zonk_predicate(&pred),
                span: span.clone(),
                givens: givens.clone(),
            });
        }

        self.wanteds.extend(zonked);
        evvars
    }
    fn with_givens<R>(
        &mut self,
        preds: Vec<Predicate>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> (Vec<GivenEvidence>, R) {
        let old_len = self.givens.len();

        let new_givens = preds
            .into_iter()
            .map(|pred| GivenEvidence {
                ev: self.fresh_evvar_id(),
                pred: self.zonk_predicate(&pred),
            })
            .collect::<Vec<_>>();

        self.givens.extend(new_givens.clone());
        let result = f(self);
        self.givens.truncate(old_len);
        (new_givens, result)
    }
    fn substitute_predicate(pred: &Predicate, subst: &FxHashMap<TyVarId, Ty>) -> Predicate {
        Predicate {
            class_id: pred.class_id,
            args: pred
                .args
                .iter()
                .map(|arg| {
                    subst
                        .iter()
                        .fold(arg.clone(), |acc, (name, ty)| acc.substitute(*name, ty))
                })
                .collect(),
        }
    }
    fn substitute_scheme(scheme: &Scheme, subst: &FxHashMap<TyVarId, Ty>) -> Scheme {
        Scheme {
            quantifiers: scheme.quantifiers.clone(),

            predicates: scheme
                .predicates
                .iter()
                .map(|pred| Self::substitute_predicate(pred, subst))
                .collect(),

            ty: subst.iter().fold(scheme.ty.clone(), |acc, (name, ty)| {
                acc.substitute(*name, ty)
            }),
        }
    }
    fn predicate_same_shape(a: &Predicate, b: &Predicate) -> bool {
        a.class_id == b.class_id && a.args == b.args
    }
    fn superclass_predicates_for_given(&self, given: &Predicate) -> Vec<Predicate> {
        let given = self.zonk_predicate(given);

        let Some(class_def) = self.tables.class_defs.get(&given.class_id) else {
            return Vec::new();
        };

        if class_def.params.len() != given.args.len() {
            return Vec::new();
        }

        let subst = class_def
            .params
            .iter()
            .zip(given.args.iter())
            .map(|((param_id, _), arg)| (*param_id, arg.clone()))
            .collect::<FxHashMap<_, _>>();

        class_def
            .superclasses
            .iter()
            .map(|pred| Self::instantiate_predicate_with_subst(pred, &subst))
            .map(|pred| self.zonk_predicate(&pred))
            .collect()
    }
    fn specialize_superclasses_for_instance(
        class_def: &ClassDefinition,
        instance_head: &Predicate,
    ) -> Vec<Predicate> {
        let subst = class_def
            .params
            .iter()
            .zip(instance_head.args.iter())
            .map(|((param_id, _), instance_arg)| (*param_id, instance_arg.clone()))
            .collect::<FxHashMap<_, _>>();

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
        instance_quantifiers: &[(TyVarId, Kind)],
    ) -> Scheme {
        let mut subst = FxHashMap::<TyVarId, Ty>::default();

        for ((param_id, _), instance_arg) in
            class_def.params.iter().zip(instance_head.args.iter())
        {
            subst.insert(*param_id, instance_arg.clone());
        }

        let specialized = Self::substitute_scheme(method_scheme, &subst);

        let specialized_class_head = Predicate {
            class_id: class_def.id,
            args: class_def
                .params
                .iter()
                .map(|(param_id, _)| subst.get(param_id).cloned().unwrap_or(Ty::TyVar(*param_id)))
                .collect(),
        };

        let mut predicates = specialized
            .predicates
            .into_iter()
            .filter(|pred| !Self::predicate_same_shape(pred, &specialized_class_head))
            .collect::<Vec<_>>();

        predicates.extend(instance_context.iter().cloned());

        let mut quantifiers = instance_quantifiers.to_vec();

        for (id, kind) in specialized.quantifiers {
            if !quantifiers.iter().any(|(existing, _)| existing == &id) {
                quantifiers.push((id, kind));
            }
        }

        Scheme {
            quantifiers,
            predicates,
            ty: specialized.ty,
        }
    }
    fn check_instance_overlaps(&self) -> Vec<TypeError> {
        instance_overlaps(self.tables.instance_defs)
            .into_iter()
            .map(|overlap| TypeError::OverlappingInstances {
                span: self.get_file_span(0..0),
                class_name: self.display_class_name(overlap.class),
                instance_type: overlap
                    .instance_args
                    .iter()
                    .map(|arg| self.pretty_type(arg))
                    .collect::<Vec<_>>()
                    .join(" "),
            })
            .collect()
    }
    fn match_instance_type(
        pattern: &Ty,
        target: &Ty,
        subst: &mut FxHashMap<TyVarId, Ty>,
    ) -> bool {
        match pattern {
            Ty::TyVar(id) => match subst.get(id) {
                Some(existing) => existing == target,
                None => {
                    subst.insert(*id, target.clone());
                    true
                }
            },

            Ty::TyCon(a) => matches!(target, Ty::TyCon(b) if a == b),
            Ty::TyUnit => matches!(target, Ty::TyUnit),

            Ty::TyApp(f1, a1) => match target {
                Ty::TyApp(f2, a2) => {
                    Self::match_instance_type(f1, f2, subst)
                        && Self::match_instance_type(a1, a2, subst)
                }
                _ => false,
            },

            Ty::TyArr(a1, b1) => match target {
                Ty::TyArr(a2, b2) => {
                    Self::match_instance_type(a1, a2, subst)
                        && Self::match_instance_type(b1, b2, subst)
                }
                _ => false,
            },

            Ty::TyForAll(_, _, body) => Self::match_instance_type(body, target, subst),

            Ty::TyExVar(name) => matches!(target, Ty::TyExVar(other) if name == other),
        }
    }
    fn match_predicate_pattern(
        pattern: &Predicate,
        target: &Predicate,
    ) -> Option<FxHashMap<TyVarId, Ty>> {
        if pattern.class_id != target.class_id || pattern.args.len() != target.args.len() {
            return None;
        }

        let mut subst = FxHashMap::<TyVarId, Ty>::default();

        for (pattern_arg, target_arg) in pattern.args.iter().zip(target.args.iter()) {
            if !Self::match_instance_type(pattern_arg, target_arg, &mut subst) {
                return None;
            }
        }

        Some(subst)
    }
    fn evidence_ty_args_for_instance(
        instance_head: &Predicate,
        instance_context: &[Predicate],
        subst: &FxHashMap<TyVarId, Ty>,
    ) -> Vec<Ty> {
        let mut vars = BTreeSet::new();

        for pred in instance_context {
            Self::free_type_vars_in_predicate(pred, &mut vars);
        }

        Self::free_type_vars_in_predicate(instance_head, &mut vars);

        vars.into_iter()
            .map(|id| subst.get(&id).cloned().unwrap_or(Ty::TyVar(id)))
            .collect()
    }
    fn solve_predicates_with_givens(
        &mut self,
        predicates: &[Predicate],
        givens: &[Predicate],
    ) -> (Vec<GivenEvidence>, Vec<Result<Ev, Unsolved>>) {
        self.with_givens(givens.to_vec(), |checker| {
            let evidence_givens = checker.givens.clone();
            predicates
                .iter()
                .map(|pred| checker.entail(&evidence_givens, pred))
                .collect()
        })
    }
    // THIH §7.3 defines entailment as superclass closure over givens followed
    // by instance matching (`bySuper` then `byInst`). C1 keeps the same search
    // order and attaches evidence to each successful derivation step.
    fn entail(&self, givens: &[GivenEvidence], wanted: &Predicate) -> Result<Ev, Unsolved> {
        self.entail_inner(givens, wanted, &mut FxHashSet::default())
    }
    fn entail_inner(
        &self,
        givens: &[GivenEvidence],
        wanted: &Predicate,
        seen: &mut FxHashSet<Predicate>,
    ) -> Result<Ev, Unsolved> {
        let wanted = self.zonk_predicate(wanted);

        for given in givens {
            if self.zonk_predicate(&given.pred) == wanted {
                return Ok(Ev::Var(given.ev));
            }
        }

        for given in givens.iter().rev() {
            if let Some(evidence) =
                self.superclass_evidence_from_given(given, &wanted, &mut FxHashSet::default())
            {
                return Ok(evidence);
            }
        }

        if !seen.insert(wanted.clone()) {
            return Err(Unsolved { pred: wanted });
        }

        for inst in self.tables.instance_defs {
            if self.disabled_instance == Some(inst.id)
                || inst.head.class_id != wanted.class_id
                || inst.head.args.len() != wanted.args.len()
            {
                continue;
            }

            let Some(subst) = Self::match_predicate_pattern(&inst.head, &wanted) else {
                continue;
            };

            let mut sub_evidence = Vec::new();
            let mut solved_context = true;

            for ctx_pred in &inst.context {
                let instantiated = Self::instantiate_predicate_with_subst(ctx_pred, &subst);
                match self.entail_inner(givens, &instantiated, seen) {
                    Ok(evidence) => sub_evidence.push(evidence),
                    Err(_) => {
                        solved_context = false;
                        break;
                    }
                }
            }

            if solved_context {
                seen.remove(&wanted);
                return Ok(Ev::Inst(
                    inst.id,
                    Self::evidence_ty_args_for_instance(&inst.head, &inst.context, &subst),
                    sub_evidence,
                ));
            }
        }

        seen.remove(&wanted);
        Err(Unsolved { pred: wanted })
    }
    fn superclass_evidence_from_given(
        &self,
        given: &GivenEvidence,
        wanted: &Predicate,
        seen: &mut FxHashSet<Predicate>,
    ) -> Option<Ev> {
        let source = self.zonk_predicate(&given.pred);
        self.superclass_evidence_from_predicate(
            &source,
            Ev::Var(given.ev),
            wanted,
            seen,
        )
    }
    fn superclass_evidence_from_predicate(
        &self,
        source: &Predicate,
        source_ev: Ev,
        wanted: &Predicate,
        seen: &mut FxHashSet<Predicate>,
    ) -> Option<Ev> {
        let source = self.zonk_predicate(source);
        let wanted = self.zonk_predicate(wanted);

        if source == wanted {
            return Some(source_ev);
        }

        if !seen.insert(source.clone()) {
            return None;
        }

        for (index, superclass_pred) in self
            .superclass_predicates_for_given(&source)
            .into_iter()
            .enumerate()
        {
            let superclass_ev = Ev::Super(Box::new(source_ev.clone()), source.class_id, index);

            if let Some(evidence) = self.superclass_evidence_from_predicate(
                &superclass_pred,
                superclass_ev,
                &wanted,
                seen,
            ) {
                seen.remove(&source);
                return Some(evidence);
            }
        }

        seen.remove(&source);
        None
    }
    fn unresolved_wanteds(&mut self) -> Vec<Wanted> {
        self.unresolved_wanteds_in_range(0..self.wanteds.len())
    }
    fn unresolved_wanteds_in_range(&mut self, range: Range<usize>) -> Vec<Wanted> {
        let mut unresolved = Vec::new();

        let wanteds = self
            .wanteds
            .get(range)
            .expect("ICE: wanted range out of bounds")
            .to_vec();
        for wanted in wanteds {
            match self.entail(&wanted.givens, &wanted.pred) {
                Ok(evidence) => {
                    self.evidence_solutions.insert(wanted.ev, evidence);
                }
                Err(_) => unresolved.push(wanted),
            }
        }

        unresolved
    }
    fn wanteds_error(&self, wanteds: Vec<Wanted>) -> TypeError {
        let span = wanteds
            .first().map_or_else(|| self.get_file_span(0..0), |w| self.get_file_span(w.span.clone()));

        TypeError::UnsatisfiedConstraints {
            span,
            predicates: wanteds
                .into_iter()
                .map(|w| self.pretty_predicate(&self.zonk_predicate(&w.pred)))
                .collect(),
        }
    }
    fn collect_exvars_in_type(ty: &Ty, acc: &mut Vec<EvarId>) {
        match ty {
            Ty::TyExVar(id) => {
                if !acc.contains(id) {
                    acc.push(*id);
                }
            }
            Ty::TyArr(lhs, rhs) | Ty::TyApp(lhs, rhs) => {
                Self::collect_exvars_in_type(lhs, acc);
                Self::collect_exvars_in_type(rhs, acc);
            }
            Ty::TyForAll(_, preds, body) => {
                for pred in preds {
                    for arg in &pred.args {
                        Self::collect_exvars_in_type(arg, acc);
                    }
                }
                Self::collect_exvars_in_type(body, acc);
            }
            Ty::TyUnit | Ty::TyVar(_) | Ty::TyCon(_) => {}
        }
    }
    fn collect_exvars_in_predicate(pred: &Predicate, acc: &mut Vec<EvarId>) {
        for arg in &pred.args {
            Self::collect_exvars_in_type(arg, acc);
        }
    }
    fn collect_fixed_exvars(ctx: &Context) -> Vec<EvarId> {
        let mut fixed = Vec::new();

        for entry in ctx.get_entries() {
            if let ContextEntry::TypedVar { ty, .. } = entry {
                Self::collect_exvars_in_type(&ctx.apply(&ty), &mut fixed);
            }
        }

        fixed
    }
    fn substitute_evars_in_predicate(
        pred: &Predicate,
        subst: &FxHashMap<EvarId, Ty>,
    ) -> Predicate {
        Predicate {
            class_id: pred.class_id,
            args: pred
                .args
                .iter()
                .map(|arg| {
                    subst
                        .iter()
                        .fold(arg.clone(), |acc, (evar, ty)| acc.substitute_evar(*evar, ty))
                })
                .collect(),
        }
    }
    fn ambiguous_tyvar_name(index: usize) -> String {
        const LETTERS: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";
        LETTERS.get(index).map_or_else(
            || format!("t{}", index - LETTERS.len()),
            |letter| char::from(*letter).to_string(),
        )
    }
    fn pretty_ambiguous_predicates(&mut self, wanteds: &[Wanted]) -> Vec<String> {
        let mut evars = Vec::new();
        for wanted in wanteds {
            Self::collect_exvars_in_predicate(&wanted.pred, &mut evars);
        }

        let scratch_checkpoint = self.delta.names.checkpoint();
        let mut subst = FxHashMap::default();
        for (index, evar) in evars.into_iter().enumerate() {
            let universal_id = self
                .delta
                .names
                .fresh_tyvar(&Self::ambiguous_tyvar_name(index));
            let replacement = Ty::TyVar(universal_id);
            subst.insert(evar, replacement);
        }
        let tables = PrettyTables::new(self.names, self.tables.tycon_table, self.tables.class_table)
            .with_name_scratch(&self.delta.names);

        let predicates = wanteds
            .iter()
            .map(|wanted| {
                let pred = Self::substitute_evars_in_predicate(&wanted.pred, &subst);
                pred.pretty_with_tables(tables)
            })
            .collect();
        self.delta.names.restore(scratch_checkpoint);
        predicates
    }
    fn split_generalized_wanteds(
        &mut self,
        ctx: &Context,
        ty_evars: &[EvarId],
        wanted_range: Range<usize>,
    ) -> Result<Vec<GivenEvidence>, TypeError> {
        let fixed_evars = Self::collect_fixed_exvars(ctx);
        let mut retained = Vec::<GivenEvidence>::new();
        let mut concrete_errors = Vec::<Wanted>::new();
        let mut ambiguous = Vec::<Wanted>::new();

        // THIH §7.4-7.5 `split`: unsolved predicates that mention variables
        // generalized by the binding become retained constraints; concrete
        // unsolved predicates remain errors, and retained predicates whose
        // variables do not appear in the inferred type are ambiguous.
        let wanteds = self
            .wanteds
            .get(wanted_range)
            .expect("ICE: generalized wanted range out of bounds")
            .to_vec();
        for wanted in wanteds {
            let pred = self.zonk_predicate(&wanted.pred);
            let wanted = Wanted { pred, ..wanted };

            let mut givens = wanted.givens.clone();
            givens.extend(retained.clone());

            if let Ok(evidence) = self.entail(&givens, &wanted.pred) {
                self.evidence_solutions.insert(wanted.ev, evidence);
                continue;
            }

            let mut pred_evars = Vec::new();
            Self::collect_exvars_in_predicate(&wanted.pred, &mut pred_evars);

            if pred_evars.is_empty()
                || pred_evars.iter().all(|evar| fixed_evars.contains(evar))
            {
                concrete_errors.push(wanted);
                continue;
            }

            if !pred_evars.iter().all(|evar| ty_evars.contains(evar)) {
                ambiguous.push(wanted);
                continue;
            }

            let given = GivenEvidence {
                ev: wanted.ev,
                pred: wanted.pred.clone(),
            };
            self.evidence_solutions.insert(wanted.ev, Ev::Var(wanted.ev));
            retained.push(given);
        }

        if !concrete_errors.is_empty() {
            return Err(self.wanteds_error(concrete_errors));
        }

        if !ambiguous.is_empty() {
            let span = ambiguous.first().map_or_else(
                || self.get_file_span(0..0),
                |wanted| self.get_file_span(wanted.span.clone()),
            );
            let predicates = self.pretty_ambiguous_predicates(&ambiguous);
            return Err(TypeError::AmbiguousConstraints {
                span,
                predicates,
            });
        }

        Ok(retained)
    }
    fn generalize_top_level_type_with_subst(
        &mut self,
        ctx: &Context,
        ty: &Ty,
        wanted_range: Range<usize>,
    ) -> Result<GeneralizedInferred, TypeError> {
        let zonked = self.zonk_type(&ctx.apply(ty));

        let mut evars = Vec::new();
        Self::collect_exvars_in_type(&zonked, &mut evars);
        let retained = self.split_generalized_wanteds(ctx, &evars, wanted_range)?;
        for given in &retained {
            Self::collect_exvars_in_predicate(&given.pred, &mut evars);
        }

        let mut result = zonked.clone();
        let mut binders = Vec::new();
        let mut subst = FxHashMap::default();

        for evar in evars {
            let (universal_id, _) = self.fresh_universal("t");
            let kind = ctx.evar_kind_id(evar).unwrap_or(Kind::Star);
            let replacement = Ty::TyVar(universal_id);

            result = result.substitute_evar(evar, &replacement);
            subst.insert(evar, replacement);
            binders.push((universal_id, kind));
        }

        let predicates = retained
            .iter()
            .map(|given| Self::substitute_evars_in_predicate(&given.pred, &subst))
            .collect::<Vec<_>>();
        result = Ty::make_forall_group(binders, predicates, result);

        let retained = retained
            .into_iter()
            .map(|given| GivenEvidence {
                ev: given.ev,
                pred: Self::substitute_evars_in_predicate(&given.pred, &subst),
            })
            .collect();

        Ok(GeneralizedInferred {
            ty: result,
            subst,
            retained,
        })
    }
    fn current_wanted_range_from(&self, start: usize) -> Range<usize> {
        start..self.wanteds.len()
    }
    fn wanted_count(&self) -> usize {
        self.wanteds.len()
    }
    fn solve_wanteds_from(&mut self, start: usize) -> Vec<Wanted> {
        self.unresolved_wanteds_in_range(start..self.wanteds.len())
    }
}

#[cfg(test)]
include!("constraints_tests.rs");
