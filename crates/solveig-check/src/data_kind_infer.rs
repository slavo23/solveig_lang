//! Kind inference for algebraic data declarations.
//! Resolver output enters here before data definitions become checked module metadata.

use solveig_base::{FxHashMap, FxHashSet};

use solveig_ast::ast::{Kind, KindMetaId, Ty, TyConId, TyVarId, tyvar_display_name};
use solveig_base::FileSpan;

use crate::signature_infer::KindError;

#[derive(Debug, Clone)]
pub struct RawDataDef {
    pub id: TyConId,
    pub name: String,
    pub short_name: String,
    pub span: FileSpan,
    pub ty_params: Vec<TyVarId>,
    pub constructors: Vec<RawConstructorDef>,
}

#[derive(Debug, Clone)]
pub struct RawConstructorDef {
    pub name: String,
    pub short_name: String,
    pub span: FileSpan,
    pub fields: Vec<Ty>,
}

#[derive(Debug, Clone)]
pub struct InferredDataDef {
    pub id: TyConId,
    pub name: String,
    pub short_name: String,
    pub ty_params: Vec<(TyVarId, Kind)>,
    pub tycon_kind: Kind,
    pub constructors: Vec<InferredConstructorDef>,
}

#[derive(Debug, Clone)]
pub struct InferredConstructorDef {
    pub name: String,
    pub short_name: String,
    pub kinded_fields: Vec<Ty>,
    pub full_type: Ty,
}

pub struct DataKindInferencer {
    meta_order: Vec<KindMetaId>,
    solutions: FxHashMap<KindMetaId, Kind>,
    next_meta: u32,
}

impl Default for DataKindInferencer {
    fn default() -> Self {
        Self::new()
    }
}

impl DataKindInferencer {
    pub fn new() -> Self {
        Self {
            meta_order: Vec::new(),
            solutions: FxHashMap::default(),
            next_meta: 0,
        }
    }

    fn fresh_meta(&mut self) -> Kind {
        let id = KindMetaId(self.next_meta);
        self.next_meta += 1;
        self.meta_order.push(id);
        Kind::make_meta(id)
    }

    fn fresh_meta_before(&mut self, target: KindMetaId) -> Kind {
        let id = KindMetaId(self.next_meta);
        self.next_meta += 1;
        let pos = self.position_of(target).unwrap_or_else(|| {
            panic!(
                "ICE: fresh_meta_before target '?k{}' not in ordering",
                target.0
            )
        });
        self.meta_order.insert(pos, id);
        Kind::make_meta(id)
    }

    fn position_of(&self, meta: KindMetaId) -> Option<usize> {
        self.meta_order.iter().position(|m| *m == meta)
    }

    fn is_before(&self, a: KindMetaId, b: KindMetaId) -> bool {
        match (self.position_of(a), self.position_of(b)) {
            (Some(pa), Some(pb)) => pa < pb,
            _ => false,
        }
    }

    fn is_meta(&self, id: KindMetaId) -> bool {
        self.meta_order.contains(&id)
    }

    fn is_unsolved(&self, meta: KindMetaId) -> bool {
        self.is_meta(meta) && !self.solutions.contains_key(&meta)
    }

    fn solve(&mut self, meta: KindMetaId, kind: Kind) {
        self.solutions.insert(meta, kind);
    }

    fn apply_predicate(&self, pred: &solveig_ast::ast::Predicate) -> solveig_ast::ast::Predicate {
        solveig_ast::ast::Predicate {
            class_id: pred.class_id,
            args: pred.args.iter().map(|arg| self.apply_type(arg)).collect(),
        }
    }

    fn apply_kind(&self, kind: &Kind) -> Kind {
        match kind {
            Kind::Star => Kind::Star,
            Kind::Meta(id) => {
                if let Some(sol) = self.solutions.get(id) {
                    self.apply_kind(sol)
                } else {
                    Kind::Meta(*id)
                }
            }
            Kind::Arrow(lhs, rhs) => Kind::make_arrow(self.apply_kind(lhs), self.apply_kind(rhs)),
        }
    }

    fn apply_type(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::TyUnit => Ty::TyUnit,
            Ty::TyVar(n) => Ty::TyVar(*n),
            Ty::TyExVar(id) => Ty::TyExVar(*id),
            Ty::TyCon(n) => Ty::TyCon(*n),
            Ty::TyArr(l, r) => Ty::make_arr(self.apply_type(l), self.apply_type(r)),
            Ty::TyApp(l, r) => Ty::make_app(self.apply_type(l), self.apply_type(r)),
            Ty::TyForAll(binders, preds, body) => Ty::make_forall_group(
                binders
                    .iter()
                    .map(|(name, kind)| (*name, self.zonk(kind)))
                    .collect(),
                preds.iter().map(|p| self.apply_predicate(p)).collect(),
                self.apply_type(body),
            ),
        }
    }

    fn zonk(&self, kind: &Kind) -> Kind {
        default_unsolved(self.apply_kind(kind))
    }

    fn default_all_unsolved(&mut self) {
        let unsolved: Vec<KindMetaId> = self
            .meta_order
            .iter()
            .copied()
            .filter(|m| !self.solutions.contains_key(m))
            .collect();
        for m in unsolved {
            self.solutions.insert(m, Kind::Star);
        }
    }

    fn promote(&mut self, target: KindMetaId, kind: &Kind) -> Result<Kind, KindError> {
        let kind = self.apply_kind(kind);
        match &kind {
            Kind::Star => Ok(Kind::Star),

            Kind::Arrow(lhs, rhs) => {
                let lhs = *lhs.clone();
                let rhs = *rhs.clone();
                let lhs_p = self.promote(target, &lhs)?;
                let rhs_applied = self.apply_kind(&rhs);
                let rhs_p = self.promote(target, &rhs_applied)?;
                Ok(Kind::make_arrow(lhs_p, rhs_p))
            }

            Kind::Meta(id) => {
                if *id == target {
                    Err(KindError::occurs_check_failed(
                        format!("?k{}", target.0),
                        kind.clone(),
                    ))
                } else if !self.is_meta(*id) || self.is_before(*id, target) {
                    Ok(kind.clone())
                } else {
                    let fresh = self.fresh_meta_before(target);
                    self.solve(*id, fresh.clone());
                    Ok(fresh)
                }
            }
        }
    }

    fn unify(&mut self, lhs: &Kind, rhs: &Kind) -> Result<(), KindError> {
        let lhs = self.apply_kind(lhs);
        let rhs = self.apply_kind(rhs);

        match (&lhs, &rhs) {
            _ if lhs == rhs => Ok(()),

            (Kind::Arrow(l1, r1), Kind::Arrow(l2, r2)) => {
                let (l1, r1, l2, r2) = (*l1.clone(), *r1.clone(), *l2.clone(), *r2.clone());
                self.unify(&l1, &l2)?;

                let r1 = self.apply_kind(&r1);
                let r2 = self.apply_kind(&r2);
                self.unify(&r1, &r2)
            }

            (Kind::Meta(id), _) if self.is_unsolved(*id) => {
                let promoted = self.promote(*id, &rhs)?;
                self.solve(*id, promoted);
                Ok(())
            }

            (_, Kind::Meta(id)) if self.is_unsolved(*id) => {
                let promoted = self.promote(*id, &lhs)?;
                self.solve(*id, promoted);
                Ok(())
            }

            _ => Err(KindError::cannot_unify(lhs, rhs)),
        }
    }

    fn infer_kind(
        &mut self,
        ty: &Ty,
        tycon_env: &FxHashMap<TyConId, Kind>,
        var_env: &FxHashMap<TyVarId, Kind>,
    ) -> Result<(Kind, Ty), KindError> {
        match ty {
            Ty::TyUnit => Ok((Kind::Star, Ty::TyUnit)),

            Ty::TyVar(id) => {
                let kind = var_env
                    .get(id)
                    .map(|k| self.apply_kind(k))
                    .ok_or_else(|| KindError::unbound_type_variable(tyvar_display_name(*id)))?;
                Ok((kind, Ty::TyVar(*id)))
            }

            Ty::TyExVar(id) => Ok((Kind::Star, Ty::TyExVar(*id))),

            Ty::TyCon(id) => {
                let kind = tycon_env
                    .get(id)
                    .map(|k| self.apply_kind(k))
                    .ok_or_else(|| KindError::unbound_type_constructor(tycon_id_display(*id)))?;
                Ok((kind, Ty::TyCon(*id)))
            }

            Ty::TyArr(lhs, rhs) => {
                let (lk, lt) = self.infer_kind(lhs, tycon_env, var_env)?;
                let (rk, rt) = self.infer_kind(rhs, tycon_env, var_env)?;
                self.unify(&lk, &Kind::Star)?;
                self.unify(&rk, &Kind::Star)?;
                Ok((Kind::Star, Ty::TyArr(Box::new(lt), Box::new(rt))))
            }

            Ty::TyApp(fun, arg) => {
                if matches!(**fun, Ty::TyForAll(_, _, _)) || matches!(**arg, Ty::TyForAll(_, _, _))
                {
                    return Err(KindError::cannot_unify(
                        Kind::Star,
                        Kind::make_arrow(Kind::Star, Kind::Star),
                    ));
                }

                let (fun_kind, fun_r) = self.infer_kind(fun, tycon_env, var_env)?;
                let (arg_kind, arg_r) = self.infer_kind(arg, tycon_env, var_env)?;
                let fun_kind = self.apply_kind(&fun_kind);
                let arg_kind = self.apply_kind(&arg_kind);
                let result_kind = self.app_kinding(fun_kind, arg_kind)?;
                Ok((result_kind, Ty::TyApp(Box::new(fun_r), Box::new(arg_r))))
            }

            Ty::TyForAll(binders, preds, body) => {
                let mut new_env = var_env.clone();
                let mut binder_metas = Vec::with_capacity(binders.len());
                for (name, _) in binders {
                    let fresh = self.fresh_meta();
                    new_env.insert(*name, fresh.clone());
                    binder_metas.push((*name, fresh));
                }

                let mut rebuilt_preds = Vec::with_capacity(preds.len());
                for pred in preds {
                    let rebuilt_args = pred
                        .args
                        .iter()
                        .map(|arg| {
                            let (arg_kind, arg_r) = self.infer_kind(arg, tycon_env, &new_env)?;
                            self.unify(&arg_kind, &Kind::Star)?;
                            Ok(arg_r)
                        })
                        .collect::<Result<Vec<_>, KindError>>()?;

                    rebuilt_preds.push(solveig_ast::ast::Predicate {
                        class_id: pred.class_id,
                        args: rebuilt_args,
                    });
                }

                let (bk, br) = self.infer_kind(body, tycon_env, &new_env)?;
                self.unify(&bk, &Kind::Star)?;

                Ok((
                    Kind::Star,
                    Ty::make_forall_group(
                        binder_metas
                            .into_iter()
                            .map(|(name, kind)| (name, self.zonk(&kind)))
                            .collect(),
                        rebuilt_preds,
                        br,
                    ),
                ))
            }
        }
    }

    fn app_kinding(&mut self, fun_kind: Kind, arg_kind: Kind) -> Result<Kind, KindError> {
        match fun_kind {
            Kind::Arrow(param_k, result_k) => {
                self.unify(&param_k, &arg_kind)?;
                Ok(self.apply_kind(&result_k))
            }

            Kind::Meta(id) if self.is_unsolved(id) => {
                let a1 = self.fresh_meta_before(id);
                let a2 = self.fresh_meta_before(id);

                self.solve(id, Kind::make_arrow(a1.clone(), a2.clone()));
                self.unify(&a1, &arg_kind)?;
                Ok(self.apply_kind(&a2))
            }

            _ => Err(KindError::cannot_unify(
                fun_kind,
                Kind::make_arrow(arg_kind, Kind::Star),
            )),
        }
    }

    fn check_constructor(
        &mut self,
        fields: &[Ty],
        return_type: &Ty,
        tycon_env: &FxHashMap<TyConId, Kind>,
        var_env: &FxHashMap<TyVarId, Kind>,
    ) -> Result<Vec<Ty>, KindError> {
        let mut rebuilt = Vec::new();
        for field in fields {
            let (fk, fr) = self.infer_kind(field, tycon_env, var_env)?;
            self.unify(&fk, &Kind::Star)?;
            rebuilt.push(fr);
        }
        let (ret_k, _) = self.infer_kind(return_type, tycon_env, var_env)?;
        self.unify(&ret_k, &Kind::Star)?;
        Ok(rebuilt)
    }

    fn infer_group(
        &mut self,
        group: &[RawDataDef],
        prior_tycons: &FxHashMap<TyConId, Kind>,
    ) -> Result<Vec<InferredDataDef>, KindError> {
        struct DtResult {
            id: TyConId,
            name: String,
            short_name: String,
            param_metas: Vec<(TyVarId, Kind)>,
            ctor_fields: Vec<(String, String, Vec<Ty>)>,
        }

        let mut tycon_metas: FxHashMap<TyConId, Kind> = FxHashMap::default();
        for def in group {
            let meta = self.fresh_meta();
            tycon_metas.insert(def.id, meta);
        }

        let mut tycon_env = prior_tycons.clone();
        tycon_env.extend(tycon_metas.clone());

        let mut dt_results: Vec<DtResult> = Vec::new();

        for def in group {
            let tycon_kind = self.apply_kind(&tycon_env[&def.id]);

            let mut param_metas = Vec::new();
            for p in &def.ty_params {
                let m = self.fresh_meta();
                param_metas.push((*p, m));
            }

            let expected = param_metas
                .iter()
                .rev()
                .fold(Kind::Star, |acc, (_, k)| Kind::make_arrow(k.clone(), acc));

            self.unify(&tycon_kind, &expected)
                .map_err(|error| error.with_span(def.span))?;

            let var_env: FxHashMap<TyVarId, Kind> = param_metas
                .iter()
                .map(|(n, m)| (*n, self.apply_kind(m)))
                .collect();

            let mut ret_ty = Ty::TyCon(def.id);
            for (pn, _) in &param_metas {
                ret_ty = Ty::make_app(ret_ty, Ty::TyVar(*pn));
            }

            let mut ctor_fields = Vec::new();
            for ctor in &def.constructors {
                let rebuilt = self
                    .check_constructor(&ctor.fields, &ret_ty, &tycon_env, &var_env)
                    .map_err(|error| error.with_span(ctor.span))?;
                ctor_fields.push((ctor.name.clone(), ctor.short_name.clone(), rebuilt));
            }

            dt_results.push(DtResult {
                id: def.id,
                name: def.name.clone(),
                short_name: def.short_name.clone(),
                param_metas,
                ctor_fields,
            });
        }

        self.default_all_unsolved();

        let mut results = Vec::new();
        for dt in &dt_results {
            let ty_params: Vec<(TyVarId, Kind)> = dt
                .param_metas
                .iter()
                .map(|(n, m)| (*n, self.zonk(m)))
                .collect();

            let tycon_kind = self.zonk(&tycon_metas[&dt.id]);

            let mut ctors = Vec::new();
            for (cn, short_cn, fields) in &dt.ctor_fields {
                let kf: Vec<Ty> = fields.iter().map(|f| self.apply_type(f)).collect();
                let full_type = build_constructor_type(dt.id, &ty_params, &kf);
                ctors.push(InferredConstructorDef {
                    name: cn.clone(),
                    short_name: short_cn.clone(),
                    kinded_fields: kf,
                    full_type,
                });
            }

            results.push(InferredDataDef {
                id: dt.id,
                name: dt.name.clone(),
                short_name: dt.short_name.clone(),
                ty_params,
                tycon_kind,
                constructors: ctors,
            });
        }

        Ok(results)
    }
}

fn build_constructor_type(tycon: TyConId, ty_params: &[(TyVarId, Kind)], fields: &[Ty]) -> Ty {
    let mut ret = Ty::TyCon(tycon);
    for (p, _) in ty_params {
        ret = Ty::make_app(ret, Ty::TyVar(*p));
    }
    let mut body = ret;
    for f in fields.iter().rev() {
        body = Ty::make_arr(f.clone(), body);
    }
    for (p, k) in ty_params.iter().rev() {
        body = Ty::make_forall_id(*p, k.clone(), body);
    }
    body
}

fn default_unsolved(kind: Kind) -> Kind {
    match kind {
        Kind::Meta(_) | Kind::Star => Kind::Star,
        Kind::Arrow(l, r) => Kind::make_arrow(default_unsolved(*l), default_unsolved(*r)),
    }
}

fn collect_tycon_refs(ty: &Ty, out: &mut FxHashSet<TyConId>) {
    match ty {
        Ty::TyCon(id) => {
            out.insert(*id);
        }
        Ty::TyArr(l, r) | Ty::TyApp(l, r) => {
            collect_tycon_refs(l, out);
            collect_tycon_refs(r, out);
        }
        Ty::TyForAll(_, preds, body) => {
            for pred in preds {
                for arg in &pred.args {
                    collect_tycon_refs(arg, out);
                }
            }
            collect_tycon_refs(body, out);
        }
        _ => {}
    }
}

pub fn dependency_analysis(defs: &[RawDataDef]) -> Vec<Vec<usize>> {
    let n = defs.len();
    if n == 0 {
        return vec![];
    }

    let idx: FxHashMap<TyConId, usize> = defs.iter().enumerate().map(|(i, d)| (d.id, i)).collect();

    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];
    for (i, def) in defs.iter().enumerate() {
        let mut refs = FxHashSet::default();
        for c in &def.constructors {
            for f in &c.fields {
                collect_tycon_refs(f, &mut refs);
            }
        }
        for r in refs {
            if let Some(&j) = idx.get(&r)
                && !adj[i].contains(&j)
            {
                adj[i].push(j);
            }
        }
    }

    crate::dependency_graph::dependency_ordered_sccs(&adj)
}

pub fn infer_all_data_kinds_with_env(
    raw_defs: &[RawDataDef],
    initial_tycon_env: FxHashMap<TyConId, Kind>,
) -> Result<(Vec<InferredDataDef>, FxHashMap<TyConId, Kind>), KindError> {
    let groups = dependency_analysis(raw_defs);

    let mut tycon_env: FxHashMap<TyConId, Kind> = initial_tycon_env;
    let mut all_results: Vec<InferredDataDef> = Vec::new();

    for group_indices in &groups {
        let group: Vec<RawDataDef> = group_indices.iter().map(|&i| raw_defs[i].clone()).collect();

        let mut inf = DataKindInferencer::new();
        let results = inf.infer_group(&group, &tycon_env)?;

        for d in &results {
            tycon_env.insert(d.id, d.tycon_kind.clone());
        }
        all_results.extend(results);
    }

    Ok((all_results, tycon_env))
}

pub fn infer_all_data_kinds(
    raw_defs: &[RawDataDef],
) -> Result<(Vec<InferredDataDef>, FxHashMap<TyConId, Kind>), KindError> {
    infer_all_data_kinds_with_env(raw_defs, FxHashMap::default())
}

fn tycon_id_display(id: TyConId) -> String {
    format!("T{}", id.0)
}

#[cfg(test)]
mod tests {
    use solveig_base::{FileId, FileSpan, Span};

    use super::*;

    fn test_span() -> FileSpan {
        FileSpan::new(FileId(0), Span::new(0, 0))
    }

    fn raw_data(id: TyConId, name: &str, fields: Vec<Ty>) -> RawDataDef {
        RawDataDef {
            id,
            name: name.to_string(),
            short_name: name.to_string(),
            span: test_span(),
            ty_params: Vec::new(),
            constructors: vec![RawConstructorDef {
                name: format!("{name}.Mk"),
                short_name: "Mk".to_string(),
                span: test_span(),
                fields,
            }],
        }
    }

    #[test]
    fn dependency_analysis_uses_tycon_ids() {
        let alpha = TyConId(41);
        let beta = TyConId(42);
        let defs = vec![
            raw_data(alpha, "sample.Alpha", vec![Ty::TyCon(beta)]),
            raw_data(beta, "sample.Beta", Vec::new()),
        ];

        assert_eq!(dependency_analysis(&defs), vec![vec![1], vec![0]]);
    }

    #[test]
    fn data_kind_inference_uses_imported_tycon_id_env() {
        let imported = TyConId(51);
        let local = TyConId(52);
        let imported_unit = Ty::make_app(Ty::TyCon(imported), Ty::TyUnit);
        let raw = raw_data(local, "sample.Box", vec![imported_unit.clone()]);

        let (defs, env) = infer_all_data_kinds_with_env(
            &[raw],
            FxHashMap::from_iter([(imported, Kind::make_arrow(Kind::Star, Kind::Star))]),
        )
        .expect("ICE: imported ID-keyed type constructor kind should resolve");

        assert_eq!(env.get(&local), Some(&Kind::Star));
        assert_eq!(defs[0].id, local);
        assert_eq!(
            defs[0].constructors[0].kinded_fields,
            vec![imported_unit.clone()]
        );
        assert_eq!(
            defs[0].constructors[0].full_type,
            Ty::make_arr(imported_unit, Ty::TyCon(local))
        );
    }
}
