use std::collections::{HashMap, HashSet};

use core::ast::{Kind, TypeExpr};

use crate::signature_infer::KindError;

#[derive(Debug, Clone)]
pub struct RawDataDef {
    pub name: String,
    pub ty_params: Vec<String>,
    pub constructors: Vec<RawConstructorDef>,
}

#[derive(Debug, Clone)]
pub struct RawConstructorDef {
    pub name: String,
    pub fields: Vec<TypeExpr>,
}

#[derive(Debug, Clone)]
pub struct InferredDataDef {
    pub name: String,
    pub ty_params: Vec<(String, Kind)>,
    pub tycon_kind: Kind,
    pub constructors: Vec<InferredConstructorDef>,
}

#[derive(Debug, Clone)]
pub struct InferredConstructorDef {
    pub name: String,
    pub kinded_fields: Vec<TypeExpr>,
    pub full_type: TypeExpr,
}

pub struct DataKindInferencer {
    meta_order: Vec<String>,
    solutions: HashMap<String, Kind>,
    next_meta: usize,
}

impl DataKindInferencer {
    pub fn new() -> Self {
        Self {
            meta_order: Vec::new(),
            solutions: HashMap::new(),
            next_meta: 0,
        }
    }

    fn fresh_meta(&mut self) -> Kind {
        let name = format!("?k{}", self.next_meta);
        self.next_meta += 1;
        self.meta_order.push(name.clone());
        Kind::Var(name)
    }

    fn fresh_meta_before(&mut self, target: &str) -> Kind {
        let name = format!("?k{}", self.next_meta);
        self.next_meta += 1;
        let pos = self
            .position_of(target)
            .unwrap_or_else(|| panic!("fresh_meta_before: target '{}' not in ordering", target));
        self.meta_order.insert(pos, name.clone());
        Kind::Var(name)
    }

    fn position_of(&self, meta: &str) -> Option<usize> {
        self.meta_order.iter().position(|m| m == meta)
    }

    fn is_before(&self, a: &str, b: &str) -> bool {
        match (self.position_of(a), self.position_of(b)) {
            (Some(pa), Some(pb)) => pa < pb,
            _ => false,
        }
    }

    fn is_meta(&self, name: &str) -> bool {
        self.meta_order.iter().any(|m| m == name)
    }

    fn is_unsolved(&self, meta: &str) -> bool {
        self.is_meta(meta) && !self.solutions.contains_key(meta)
    }

    fn solve(&mut self, meta: &str, kind: Kind) {
        self.solutions.insert(meta.to_string(), kind);
    }

    fn apply_kind(&self, kind: &Kind) -> Kind {
        match kind {
            Kind::Star => Kind::Star,
            Kind::Var(v) => {
                if let Some(sol) = self.solutions.get(v) {
                    self.apply_kind(sol)
                } else {
                    Kind::Var(v.clone())
                }
            }
            Kind::Arrow(lhs, rhs) => Kind::make_arrow(self.apply_kind(lhs), self.apply_kind(rhs)),
        }
    }

    fn apply_type(&self, ty: &TypeExpr) -> TypeExpr {
        match ty {
            TypeExpr::TyUnit => TypeExpr::TyUnit,
            TypeExpr::TyVar(n) => TypeExpr::TyVar(n.clone()),
            TypeExpr::TyExVar(n) => TypeExpr::TyExVar(n.clone()),
            TypeExpr::TyCon(n) => TypeExpr::TyCon(n.clone()),
            TypeExpr::TyArr(l, r) => TypeExpr::make_arr(self.apply_type(l), self.apply_type(r)),
            TypeExpr::TyApp(l, r) => TypeExpr::make_app(self.apply_type(l), self.apply_type(r)),
            TypeExpr::TyForAll(name, kind, body) => {
                TypeExpr::make_forall(name.clone(), self.zonk(kind), self.apply_type(body))
            }
        }
    }

    fn zonk(&self, kind: &Kind) -> Kind {
        default_unsolved(self.apply_kind(kind))
    }

    fn default_all_unsolved(&mut self) {
        let unsolved: Vec<String> = self
            .meta_order
            .iter()
            .filter(|m| !self.solutions.contains_key(*m))
            .cloned()
            .collect();
        for m in unsolved {
            self.solutions.insert(m, Kind::Star);
        }
    }

    fn promote(&mut self, target: &str, kind: &Kind) -> Result<Kind, KindError> {
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

            Kind::Var(v) => {
                if v == target {
                    Err(KindError::OccursCheckFailed {
                        var: target.to_string(),
                        in_kind: kind.clone(),
                    })
                } else if !self.is_meta(v) {
                    Ok(kind.clone())
                } else if self.is_before(v, target) {
                    Ok(kind.clone())
                } else {
                    let v = v.clone();
                    let fresh = self.fresh_meta_before(target);
                    self.solve(&v, fresh.clone());
                    Ok(fresh)
                }
            }
        }
    }

    fn unify(&mut self, lhs: Kind, rhs: Kind) -> Result<(), KindError> {
        let lhs = self.apply_kind(&lhs);
        let rhs = self.apply_kind(&rhs);

        match (&lhs, &rhs) {
            _ if lhs == rhs => Ok(()),

            (Kind::Arrow(l1, r1), Kind::Arrow(l2, r2)) => {
                let (l1, r1, l2, r2) = (*l1.clone(), *r1.clone(), *l2.clone(), *r2.clone());
                self.unify(l1, l2)?;

                let r1 = self.apply_kind(&r1);
                let r2 = self.apply_kind(&r2);
                self.unify(r1, r2)
            }

            (Kind::Var(v), _) if self.is_unsolved(v) => {
                let v = v.clone();
                let promoted = self.promote(&v, &rhs)?;
                self.solve(&v, promoted);
                Ok(())
            }

            (_, Kind::Var(v)) if self.is_unsolved(v) => {
                let v = v.clone();
                let promoted = self.promote(&v, &lhs)?;
                self.solve(&v, promoted);
                Ok(())
            }

            _ => Err(KindError::CannotUnify { lhs, rhs }),
        }
    }

    fn infer_kind(
        &mut self,
        ty: &TypeExpr,
        tycon_env: &HashMap<String, Kind>,
        var_env: &HashMap<String, Kind>,
    ) -> Result<(Kind, TypeExpr), KindError> {
        match ty {
            TypeExpr::TyUnit => Ok((Kind::Star, TypeExpr::TyUnit)),

            TypeExpr::TyVar(name) => {
                let kind = var_env
                    .get(name)
                    .map(|k| self.apply_kind(k))
                    .ok_or_else(|| KindError::UnboundTypeVariable(name.clone()))?;
                Ok((kind, TypeExpr::TyVar(name.clone())))
            }

            TypeExpr::TyExVar(name) => {
                let kind = var_env
                    .get(name)
                    .map(|k| self.apply_kind(k))
                    .ok_or_else(|| KindError::UnboundTypeVariable(name.clone()))?;
                Ok((kind, TypeExpr::TyExVar(name.clone())))
            }

            TypeExpr::TyCon(name) => {
                let kind = tycon_env
                    .get(name)
                    .map(|k| self.apply_kind(k))
                    .ok_or_else(|| KindError::UnboundTypeConstructor(name.clone()))?;
                Ok((kind, TypeExpr::TyCon(name.clone())))
            }

            TypeExpr::TyArr(lhs, rhs) => {
                let (lk, lt) = self.infer_kind(lhs, tycon_env, var_env)?;
                let (rk, rt) = self.infer_kind(rhs, tycon_env, var_env)?;
                self.unify(lk, Kind::Star)?;
                self.unify(rk, Kind::Star)?;
                Ok((Kind::Star, TypeExpr::TyArr(Box::new(lt), Box::new(rt))))
            }

            TypeExpr::TyApp(fun, arg) => {
                let (fun_kind, fun_r) = self.infer_kind(fun, tycon_env, var_env)?;
                let (arg_kind, arg_r) = self.infer_kind(arg, tycon_env, var_env)?;
                let fun_kind = self.apply_kind(&fun_kind);
                let arg_kind = self.apply_kind(&arg_kind);
                let result_kind = self.app_kinding(fun_kind, arg_kind)?;
                Ok((
                    result_kind,
                    TypeExpr::TyApp(Box::new(fun_r), Box::new(arg_r)),
                ))
            }

            TypeExpr::TyForAll(v, _old_k, body) => {
                let fresh = self.fresh_meta();
                let mut new_env = var_env.clone();
                new_env.insert(v.clone(), fresh.clone());
                let (bk, br) = self.infer_kind(body, tycon_env, &new_env)?;
                self.unify(bk, Kind::Star)?;
                let solved_k = self.zonk(&fresh);
                Ok((
                    Kind::Star,
                    TypeExpr::TyForAll(v.clone(), solved_k, Box::new(br)),
                ))
            }
        }
    }

    fn app_kinding(&mut self, fun_kind: Kind, arg_kind: Kind) -> Result<Kind, KindError> {
        match fun_kind {
            Kind::Arrow(param_k, result_k) => {
                self.unify(*param_k, arg_kind)?;
                Ok(self.apply_kind(&result_k))
            }

            Kind::Var(ref v) if self.is_unsolved(v) => {
                let v = v.clone();
                let a1 = self.fresh_meta_before(&v);
                let a2 = self.fresh_meta_before(&v);

                self.solve(&v, Kind::make_arrow(a1.clone(), a2.clone()));
                self.unify(a1, arg_kind)?;
                Ok(self.apply_kind(&a2))
            }

            _ => Err(KindError::CannotUnify {
                lhs: fun_kind,
                rhs: Kind::make_arrow(arg_kind, Kind::Star),
            }),
        }
    }

    fn check_constructor(
        &mut self,
        fields: &[TypeExpr],
        return_type: &TypeExpr,
        tycon_env: &HashMap<String, Kind>,
        var_env: &HashMap<String, Kind>,
    ) -> Result<Vec<TypeExpr>, KindError> {
        let mut rebuilt = Vec::new();
        for field in fields {
            let (fk, fr) = self.infer_kind(field, tycon_env, var_env)?;
            self.unify(fk, Kind::Star)?;
            rebuilt.push(fr);
        }
        let (ret_k, _) = self.infer_kind(return_type, tycon_env, var_env)?;
        self.unify(ret_k, Kind::Star)?;
        Ok(rebuilt)
    }

    fn infer_group(
        &mut self,
        group: &[RawDataDef],
        prior_tycons: &HashMap<String, Kind>,
    ) -> Result<Vec<InferredDataDef>, KindError> {
        let mut tycon_metas: HashMap<String, Kind> = HashMap::new();
        for def in group {
            let meta = self.fresh_meta();
            tycon_metas.insert(def.name.clone(), meta);
        }

        let mut tycon_env = prior_tycons.clone();
        tycon_env.extend(tycon_metas.clone());

        struct DtResult {
            name: String,
            param_metas: Vec<(String, Kind)>,
            ctor_fields: Vec<(String, Vec<TypeExpr>)>,
        }

        let mut dt_results: Vec<DtResult> = Vec::new();

        for def in group {
            let tycon_kind = self.apply_kind(&tycon_env[&def.name]);

            let mut param_metas = Vec::new();
            for p in &def.ty_params {
                let m = self.fresh_meta();
                param_metas.push((p.clone(), m));
            }

            let expected = param_metas
                .iter()
                .rev()
                .fold(Kind::Star, |acc, (_, k)| Kind::make_arrow(k.clone(), acc));

            self.unify(tycon_kind, expected)?;

            let var_env: HashMap<String, Kind> = param_metas
                .iter()
                .map(|(n, m)| (n.clone(), self.apply_kind(m)))
                .collect();

            let mut ret_ty = TypeExpr::TyCon(def.name.clone());
            for (pn, _) in &param_metas {
                ret_ty = TypeExpr::make_app(ret_ty, TypeExpr::TyVar(pn.clone()));
            }

            let mut ctor_fields = Vec::new();
            for ctor in &def.constructors {
                let rebuilt =
                    self.check_constructor(&ctor.fields, &ret_ty, &tycon_env, &var_env)?;
                ctor_fields.push((ctor.name.clone(), rebuilt));
            }

            dt_results.push(DtResult {
                name: def.name.clone(),
                param_metas,
                ctor_fields,
            });
        }

        self.default_all_unsolved();

        let mut results = Vec::new();
        for dt in &dt_results {
            let ty_params: Vec<(String, Kind)> = dt
                .param_metas
                .iter()
                .map(|(n, m)| (n.clone(), self.zonk(m)))
                .collect();

            let tycon_kind = self.zonk(&tycon_metas[&dt.name]);

            let mut ctors = Vec::new();
            for (cn, fields) in &dt.ctor_fields {
                let kf: Vec<TypeExpr> = fields.iter().map(|f| self.apply_type(f)).collect();
                let full_type = build_constructor_type(&dt.name, &ty_params, &kf);
                ctors.push(InferredConstructorDef {
                    name: cn.clone(),
                    kinded_fields: kf,
                    full_type,
                });
            }

            results.push(InferredDataDef {
                name: dt.name.clone(),
                ty_params,
                tycon_kind,
                constructors: ctors,
            });
        }

        Ok(results)
    }
}

fn build_constructor_type(
    tycon: &str,
    ty_params: &[(String, Kind)],
    fields: &[TypeExpr],
) -> TypeExpr {
    let mut ret = TypeExpr::TyCon(tycon.to_string());
    for (p, _) in ty_params {
        ret = TypeExpr::make_app(ret, TypeExpr::make_var(p.clone()));
    }
    let mut body = ret;
    for f in fields.iter().rev() {
        body = TypeExpr::make_arr(f.clone(), body);
    }
    for (p, k) in ty_params.iter().rev() {
        body = TypeExpr::make_forall(p.clone(), k.clone(), body);
    }
    body
}

fn default_unsolved(kind: Kind) -> Kind {
    match kind {
        Kind::Var(_) => Kind::Star,
        Kind::Star => Kind::Star,
        Kind::Arrow(l, r) => Kind::make_arrow(default_unsolved(*l), default_unsolved(*r)),
    }
}

fn collect_tycon_refs(ty: &TypeExpr, out: &mut HashSet<String>) {
    match ty {
        TypeExpr::TyCon(n) => {
            out.insert(n.clone());
        }
        TypeExpr::TyArr(l, r) | TypeExpr::TyApp(l, r) => {
            collect_tycon_refs(l, out);
            collect_tycon_refs(r, out);
        }
        TypeExpr::TyForAll(_, _, body) => collect_tycon_refs(body, out),
        _ => {}
    }
}

fn tarjan_scc(n: usize, adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    struct St {
        idx: usize,
        stack: Vec<usize>,
        on_stack: Vec<bool>,
        num: Vec<Option<usize>>,
        low: Vec<usize>,
        out: Vec<Vec<usize>>,
    }
    fn go(v: usize, adj: &[Vec<usize>], s: &mut St) {
        s.num[v] = Some(s.idx);
        s.low[v] = s.idx;
        s.idx += 1;
        s.stack.push(v);
        s.on_stack[v] = true;
        for &w in &adj[v] {
            if s.num[w].is_none() {
                go(w, adj, s);
                s.low[v] = s.low[v].min(s.low[w]);
            } else if s.on_stack[w] {
                s.low[v] = s.low[v].min(s.num[w].unwrap());
            }
        }
        if s.low[v] == s.num[v].unwrap() {
            let mut scc = Vec::new();
            loop {
                let w = s.stack.pop().unwrap();
                s.on_stack[w] = false;
                scc.push(w);
                if w == v {
                    break;
                }
            }
            s.out.push(scc);
        }
    }
    let mut s = St {
        idx: 0,
        stack: vec![],
        on_stack: vec![false; n],
        num: vec![None; n],
        low: vec![0; n],
        out: vec![],
    };
    for v in 0..n {
        if s.num[v].is_none() {
            go(v, adj, &mut s);
        }
    }
    s.out
}

pub fn dependency_analysis(defs: &[RawDataDef]) -> Vec<Vec<usize>> {
    let n = defs.len();
    if n == 0 {
        return vec![];
    }

    let idx: HashMap<&str, usize> = defs
        .iter()
        .enumerate()
        .map(|(i, d)| (d.name.as_str(), i))
        .collect();

    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];
    for (i, def) in defs.iter().enumerate() {
        let mut refs = HashSet::new();
        for c in &def.constructors {
            for f in &c.fields {
                collect_tycon_refs(f, &mut refs);
            }
        }
        for r in refs {
            if let Some(&j) = idx.get(r.as_str()) {
                if !adj[i].contains(&j) {
                    adj[i].push(j);
                }
            }
        }
    }

    tarjan_scc(n, &adj)
}

pub fn infer_all_data_kinds_with_env(
    raw_defs: Vec<RawDataDef>,
    initial_tycon_env: HashMap<String, Kind>,
) -> Result<(Vec<InferredDataDef>, HashMap<String, Kind>), KindError> {
    let groups = dependency_analysis(&raw_defs);

    let mut tycon_env: HashMap<String, Kind> = initial_tycon_env;
    let mut all_results: Vec<InferredDataDef> = Vec::new();

    for group_indices in &groups {
        let group: Vec<RawDataDef> = group_indices.iter().map(|&i| raw_defs[i].clone()).collect();

        let mut inf = DataKindInferencer::new();
        let results = inf.infer_group(&group, &tycon_env)?;

        for d in &results {
            tycon_env.insert(d.name.clone(), d.tycon_kind.clone());
        }
        all_results.extend(results);
    }

    Ok((all_results, tycon_env))
}

pub fn infer_all_data_kinds(
    raw_defs: Vec<RawDataDef>,
) -> Result<(Vec<InferredDataDef>, HashMap<String, Kind>), KindError> {
    infer_all_data_kinds_with_env(raw_defs, HashMap::new())
}
