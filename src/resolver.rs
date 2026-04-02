use std::collections::HashMap;

use base::utils::span::Span;
use core::ast::{
    CaseArm, CaseArmNode, ConstructorDef, DataDefinition, Kind, Pattern, PatternNode,
    ResolvedModule, TermDefinition, TermExpr, TermExprNode, TypeExpr,
};
use surface::ast::{
    SpannedWrapper, SurfaceDefinition, SurfaceExpr, SurfaceMatchArm, SurfacePattern,
    SurfaceProgram, SurfaceTy,
};

use syntax::data_kind_infer::{infer_all_data_kinds, RawConstructorDef, RawDataDef};
use syntax::signature_infer::{KindError, SignatureKindInferencer};

pub struct Resolver;

impl Resolver {
    pub fn new() -> Self {
        Self
    }

    pub fn resolve_surface_type<'src>(&self, sf: &SpannedWrapper<SurfaceTy<'src>>) -> TypeExpr {
        match &**sf {
            SurfaceTy::Var(var_name) => TypeExpr::make_var(var_name.to_string()),
            SurfaceTy::Arr(lhs, rhs) => TypeExpr::make_arr(
                self.resolve_surface_type(lhs),
                self.resolve_surface_type(rhs),
            ),
            SurfaceTy::ForAll(quants, body) => quants
                .iter()
                .rev()
                .fold(self.resolve_surface_type(body), |acc, quant| {
                    TypeExpr::make_forall((*quant).to_string(), Kind::Star, acc)
                }),
            SurfaceTy::Unit => TypeExpr::TyUnit,
            SurfaceTy::Con(c) => TypeExpr::TyCon(c.to_string()),
            SurfaceTy::App(l, r) => {
                TypeExpr::make_app(self.resolve_surface_type(l), self.resolve_surface_type(r))
            }
        }
    }

    fn resolve_surface_pattern<'src>(
        &self,
        sf: &SpannedWrapper<SurfacePattern<'src>>,
    ) -> PatternNode<Span> {
        let span: Span = sf.span.into();

        let node = match &**sf {
            SurfacePattern::Var(v) => Pattern::Var(v.to_string()),
            SurfacePattern::Wild => Pattern::Wild,
            SurfacePattern::Con(con_name, args) => Pattern::Con(
                con_name.to_string(),
                args.iter()
                    .map(|arg| self.resolve_surface_pattern(arg))
                    .collect(),
            ),
        };

        PatternNode::new(node, span)
    }
    fn resolve_surface_case_arm<'src>(
        &mut self,
        sf: &SpannedWrapper<SurfaceMatchArm<'src>>,
    ) -> CaseArmNode<Span> {
        CaseArmNode::new(
            CaseArm {
                pattern: self.resolve_surface_pattern(&sf.pattern),
                expr: self.resolve_surface_expr(&sf.expr),
            },
            sf.span.into(),
        )
    }

    fn resolve_surface_expr<'src>(
        &mut self,
        sf: &SpannedWrapper<SurfaceExpr<'src>>,
    ) -> TermExprNode<Span> {
        let span: Span = sf.span.into();
        let node = match &**sf {
            SurfaceExpr::Unit => TermExpr::Unit,
            SurfaceExpr::Var(v) => TermExpr::Var(v.to_string()),
            SurfaceExpr::Con(c) => TermExpr::Con(c.to_string()),
            SurfaceExpr::App(l, r) => TermExpr::App(
                Box::new(self.resolve_surface_expr(l)),
                Box::new(self.resolve_surface_expr(r)),
            ),
            SurfaceExpr::Ann(e, t) => TermExpr::Anno(
                Box::new(self.resolve_surface_expr(e)),
                self.resolve_surface_type(t),
            ),
            SurfaceExpr::Abs(params, body) => {
                let mut body_node = self.resolve_surface_expr(body);

                for (param_name, param_ty_anno) in params.iter().rev() {
                    let param_ty = param_ty_anno
                        .as_ref()
                        .map(|ty| self.resolve_surface_type(ty));

                    body_node = TermExprNode::new(
                        TermExpr::Abs(param_name.to_string(), param_ty, Box::new(body_node)),
                        span.clone(),
                    );
                }

                return body_node;
            }
            SurfaceExpr::IfThenElse(c, t, e) => TermExpr::IfThenElse(
                Box::new(self.resolve_surface_expr(c)),
                Box::new(self.resolve_surface_expr(t)),
                Box::new(self.resolve_surface_expr(e)),
            ),
            SurfaceExpr::Case(scr, arms) => TermExpr::Case(
                Box::new(self.resolve_surface_expr(scr)),
                arms.iter()
                    .map(|a| self.resolve_surface_case_arm(a))
                    .collect(),
            ),
            _ => todo!("unacceptable node to resolve"),
        };
        TermExprNode::new(node, span)
    }

    fn peel_foralls<'a>(&self, ty: &'a TypeExpr) -> (Vec<(String, Kind)>, &'a TypeExpr) {
        let mut bs = Vec::new();
        let mut cur = ty;
        while let TypeExpr::TyForAll(n, k, body) = cur {
            bs.push((n.clone(), k.clone()));
            cur = body;
        }
        (bs, cur)
    }

    fn peel_arrow<'a>(&self, ty: &'a TypeExpr) -> Option<(&'a TypeExpr, &'a TypeExpr)> {
        match ty {
            TypeExpr::TyArr(l, r) => Some((l, r)),
            _ => None,
        }
    }

    fn wrap_function_body_without_signature<'src>(
        &mut self,
        body: &SpannedWrapper<SurfaceExpr<'src>>,
        params: &Vec<SpannedWrapper<&'src str>>,
    ) -> TermExprNode<Span> {
        let span: Span = body.span.into();
        let mut body_node = self.resolve_surface_expr(body);

        for p in params.iter().rev() {
            body_node = TermExprNode::new(
                TermExpr::Abs(p.to_string(), None, Box::new(body_node)),
                span.clone(),
            );
        }

        body_node
    }

    fn wrap_function_body_with_signature<'src>(
        &mut self,
        sig: &TypeExpr,
        body: &SpannedWrapper<SurfaceExpr<'src>>,
        params: &Vec<SpannedWrapper<&'src str>>,
    ) -> TermExprNode<Span> {
        let span: Span = body.span.into();
        let mut body_node = self.resolve_surface_expr(body);
        let (quants, after) = self.peel_foralls(sig);

        let mut remaining = after;
        let mut annos: Vec<(String, TypeExpr)> = Vec::new();
        for p in params.iter() {
            let (arg_ty, ret_ty) = self
                .peel_arrow(remaining)
                .unwrap_or_else(|| panic!("parameter mismatch for {:#?}", *p));
            annos.push((p.to_string(), arg_ty.clone()));
            remaining = ret_ty;
        }

        for (pn, pt) in annos.into_iter().rev() {
            body_node = TermExprNode::new(
                TermExpr::Abs(pn, Some(pt), Box::new(body_node)),
                span.clone(),
            );
        }
        for (qn, qk) in quants.into_iter().rev() {
            body_node =
                TermExprNode::new(TermExpr::TyAbs(qn, qk, Box::new(body_node)), span.clone());
        }
        body_node
    }

    fn resolve_surface_data_def<'src>(
        &self,
        name: &str,
        ty_params: &[SpannedWrapper<&'src str>],
        ctors: &[SpannedWrapper<surface::ast::SufraceConstructor<'src>>],
    ) -> RawDataDef {
        RawDataDef {
            name: name.to_string(),
            ty_params: ty_params.iter().map(|p| (*p).to_string()).collect(),
            constructors: ctors
                .iter()
                .map(|c| RawConstructorDef {
                    name: (*c.name).to_string(),
                    fields: c
                        .params
                        .iter()
                        .map(|t| self.resolve_surface_type(t))
                        .collect(),
                })
                .collect(),
        }
    }

    pub fn resolve_surface_program<'src>(
        mut self,
        sf_prog: SurfaceProgram<'src>,
    ) -> Result<ResolvedModule<Span>, KindError> {
        let mut raw_data_defs = Vec::new();
        for defn in sf_prog.iter() {
            if let SurfaceDefinition::DataDef {
                name,
                ty_params,
                constructors,
            } = &**defn
            {
                raw_data_defs.push(self.resolve_surface_data_def(
                    &(**name).to_string(),
                    ty_params,
                    constructors,
                ));
            }
        }

        let (inferred_data, tycon_kinds) = infer_all_data_kinds(raw_data_defs)?;

        let mut data_defs = Vec::new();
        let mut constructor_types: HashMap<String, TypeExpr> = HashMap::new();

        for idef in &inferred_data {
            let mut core_ctors = Vec::new();
            for ctor in &idef.constructors {
                constructor_types.insert(ctor.name.clone(), ctor.full_type.clone());
                core_ctors.push(ConstructorDef {
                    name: ctor.name.clone(),
                    fields: ctor.kinded_fields.clone(),
                    full_type: ctor.full_type.clone(),
                });
            }
            data_defs.push(DataDefinition {
                name: idef.name.clone(),
                ty_params: idef.ty_params.clone(),
                tycon_kind: idef.tycon_kind.clone(),
                constructors: core_ctors,
            });
        }

        let mut func_types: HashMap<String, TypeExpr> = HashMap::new();

        for defn in sf_prog.iter() {
            if let SurfaceDefinition::SigDef { name, ty } = &**defn {
                let raw_sig = self.resolve_surface_type(ty);
                let inf = SignatureKindInferencer::with_tycon_env(tycon_kinds.clone());
                let info = inf.infer_signature(&raw_sig)?;
                func_types.insert((**name).to_string(), info.kinded_signature.clone());
            }
        }

        let mut func_defs = Vec::new();

        for defn in sf_prog.iter() {
            if let SurfaceDefinition::FunDef { name, body, params } = &**defn {
                let fn_name = (**name).to_string();
                let sig = func_types.get(&fn_name).cloned();

                let wrapped = match &sig {
                    Some(sig) => self.wrap_function_body_with_signature(sig, body, params),
                    None => self.wrap_function_body_without_signature(body, params),
                };

                func_defs.push(TermDefinition {
                    name: fn_name,
                    signature: sig,
                    body: wrapped,
                });
            }
        }

        Ok(ResolvedModule {
            data_defs,
            tycon_kinds,
            constructor_types,
            func_types,
            func_defs,
        })
    }
}
