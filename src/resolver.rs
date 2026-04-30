use std::collections::HashMap;

use base::utils::span::Span;
use core::ast::{
    BinOp, CaseArm, CaseArmNode, ConstructorDef, DataDefinition, Kind, Literal, Pattern,
    PatternNode, ResolvedModule, TermDefinition, TermExpr, TermExprNode, TypeExpr, UnOp,
};
use surface::ast::{
    ModuleName, NameRef, ParsedModule, SpannedWrapper, SurfaceBinOp, SurfaceDefinition,
    SurfaceExpr, SurfaceLiteral, SurfaceMatchArm, SurfacePattern, SurfaceTy, SurfaceUnOp,
};

use syntax::data_kind_infer::{infer_all_data_kinds_with_env, RawConstructorDef, RawDataDef};
use syntax::signature_infer::{KindError, SignatureKindInferencer};

use crate::project::ModuleContext;

fn qualify(module: &ModuleName, name: &str) -> String {
    format!("{}.{}", module.as_string(), name)
}

#[derive(Debug, Clone, Default)]
pub struct ResolverImports {
    pub tycon_kinds: HashMap<String, Kind>,
}

pub struct Resolver;

impl Resolver {
    pub fn new() -> Self {
        Self
    }

    fn resolve_term_ref<'src>(&self, r: &NameRef<'src>, cx: &ModuleContext) -> String {
        match r.segments.as_slice() {
            [name] => {
                if let Some(sym) = cx.builtins.terms.get(*name) {
                    return sym.display();
                }
                if let Some(sym) = cx.local_api.terms.get(*name) {
                    return sym.display();
                }

                let mut found: Option<String> = None;
                for imp in &cx.imported_apis {
                    if let Some(sym) = imp.api.terms.get(*name) {
                        let disp = sym.display();
                        if let Some(prev) = &found {
                            if prev != &disp {
                                panic!("ambiguous imported term '{}'", name);
                            }
                        } else {
                            found = Some(disp);
                        }
                    }
                }

                found.unwrap_or_else(|| name.to_string())
            }

            [first, rest @ ..] => {
                if let Some(imp) = cx.imported_apis.iter().find(|i| i.alias == *first) {
                    let local_name = *rest.last().unwrap();
                    if let Some(sym) = imp.api.terms.get(local_name) {
                        return sym.display();
                    }
                    panic!("unknown imported term {}.{}", first, local_name);
                }

                let module = r.segments[..r.segments.len() - 1].join(".");
                let name = r.last();
                format!("{}.{}", module, name)
            }

            [] => panic!("empty name ref"),
        }
    }

    fn resolve_tycon_ref<'src>(&self, r: &NameRef<'src>, cx: &ModuleContext) -> String {
        match r.segments.as_slice() {
            [name] => {
                if let Some(sym) = cx.builtins.types.get(*name) {
                    return sym.display();
                }
                if let Some(sym) = cx.local_api.types.get(*name) {
                    return sym.display();
                }

                let mut found: Option<String> = None;
                for imp in &cx.imported_apis {
                    if let Some(sym) = imp.api.types.get(*name) {
                        let disp = sym.display();
                        if let Some(prev) = &found {
                            if prev != &disp {
                                panic!("ambiguous imported type '{}'", name);
                            }
                        } else {
                            found = Some(disp);
                        }
                    }
                }

                found.unwrap_or_else(|| name.to_string())
            }

            [first, rest @ ..] => {
                if let Some(imp) = cx.imported_apis.iter().find(|i| i.alias == *first) {
                    let local_name = *rest.last().unwrap();
                    if let Some(sym) = imp.api.types.get(local_name) {
                        return sym.display();
                    }
                    panic!("unknown imported type {}.{}", first, local_name);
                }

                let module = r.segments[..r.segments.len() - 1].join(".");
                let name = r.last();
                format!("{}.{}", module, name)
            }

            [] => panic!("empty name ref"),
        }
    }

    fn resolve_ctor_ref<'src>(&self, r: &NameRef<'src>, cx: &ModuleContext) -> String {
        match r.segments.as_slice() {
            [name] => {
                if let Some(sym) = cx.builtins.constructors.get(*name) {
                    return sym.display();
                }
                if let Some(sym) = cx.local_api.constructors.get(*name) {
                    return sym.display();
                }

                let mut found: Option<String> = None;
                for imp in &cx.imported_apis {
                    if let Some(sym) = imp.api.constructors.get(*name) {
                        let disp = sym.display();
                        if let Some(prev) = &found {
                            if prev != &disp {
                                panic!("ambiguous imported constructor '{}'", name);
                            }
                        } else {
                            found = Some(disp);
                        }
                    }
                }

                found.unwrap_or_else(|| name.to_string())
            }

            [first, rest @ ..] => {
                if let Some(imp) = cx.imported_apis.iter().find(|i| i.alias == *first) {
                    let local_name = *rest.last().unwrap();
                    if let Some(sym) = imp.api.constructors.get(local_name) {
                        return sym.display();
                    }
                    panic!("unknown imported constructor {}.{}", first, local_name);
                }

                let module = r.segments[..r.segments.len() - 1].join(".");
                let name = r.last();
                format!("{}.{}", module, name)
            }

            [] => panic!("empty name ref"),
        }
    }

    pub fn resolve_surface_type_in_module<'src>(
        &self,
        sf: &SpannedWrapper<SurfaceTy<'src>>,
        cx: &ModuleContext,
    ) -> TypeExpr {
        match &sf.0 {
            SurfaceTy::Var(var_name) => TypeExpr::make_var(var_name.to_string()),
            SurfaceTy::Arr(lhs, rhs) => TypeExpr::make_arr(
                self.resolve_surface_type_in_module(&lhs, cx),
                self.resolve_surface_type_in_module(&rhs, cx),
            ),
            SurfaceTy::ForAll(quants, body) => quants.iter().rev().fold(
                self.resolve_surface_type_in_module(&body, cx),
                |acc, quant| TypeExpr::make_forall((*quant).0.to_string(), Kind::Star, acc),
            ),
            SurfaceTy::Unit => TypeExpr::TyUnit,
            SurfaceTy::Con(c) => TypeExpr::TyCon(self.resolve_tycon_ref(&c.0, cx)),
            SurfaceTy::App(l, r) => TypeExpr::make_app(
                self.resolve_surface_type_in_module(&l, cx),
                self.resolve_surface_type_in_module(&r, cx),
            ),
        }
    }

    fn resolve_surface_pattern_in_module<'src>(
        &self,
        sf: &SpannedWrapper<SurfacePattern<'src>>,
        cx: &ModuleContext,
    ) -> PatternNode<Span> {
        let span: Span = sf.1.into();

        let node = match &sf.0 {
            SurfacePattern::Var(v) => Pattern::Var(v.to_string()),
            SurfacePattern::Wild => Pattern::Wild,
            SurfacePattern::Con(con_name, args) => Pattern::Con(
                self.resolve_ctor_ref(&con_name.0, cx),
                args.iter()
                    .map(|arg| self.resolve_surface_pattern_in_module(arg, cx))
                    .collect(),
            ),
        };

        PatternNode::new(node, span)
    }

    fn resolve_surface_expr_in_module<'src>(
        &mut self,
        sf: &SpannedWrapper<SurfaceExpr<'src>>,
        cx: &ModuleContext,
    ) -> TermExprNode<Span> {
        let span: Span = sf.1.into();

        let node = match &sf.0 {
            SurfaceExpr::Unit => TermExpr::Unit,
            SurfaceExpr::Hole => TermExpr::Hole,
            SurfaceExpr::Var(v) => TermExpr::Var(self.resolve_term_ref(&v.0, cx)),
            SurfaceExpr::Con(c) => TermExpr::Con(self.resolve_ctor_ref(&c.0, cx)),
            SurfaceExpr::Literal(lit) => match lit {
                SurfaceLiteral::Int(i_val) => TermExpr::Lit(Literal::Int(i_val.clone())),
            },
            SurfaceExpr::App(l, r) => TermExpr::App(
                Box::new(self.resolve_surface_expr_in_module(&l, cx)),
                Box::new(self.resolve_surface_expr_in_module(&r, cx)),
            ),
            SurfaceExpr::Ann(e, t) => TermExpr::Anno(
                Box::new(self.resolve_surface_expr_in_module(&e, cx)),
                self.resolve_surface_type_in_module(&t, cx),
            ),
            SurfaceExpr::Abs(params, body) => {
                let mut body_node = self.resolve_surface_expr_in_module(&body, cx);

                for (param_name, param_ty_anno) in params.iter().rev() {
                    let param_ty = param_ty_anno
                        .as_ref()
                        .map(|ty| self.resolve_surface_type_in_module(ty, cx));

                    body_node = TermExprNode::new(
                        TermExpr::Abs(param_name.0.to_string(), param_ty, Box::new(body_node)),
                        span.clone(),
                    );
                }

                return body_node;
            }
            SurfaceExpr::IfThenElse(c, t, e) => TermExpr::IfThenElse(
                Box::new(self.resolve_surface_expr_in_module(&c, cx)),
                Box::new(self.resolve_surface_expr_in_module(&t, cx)),
                Box::new(self.resolve_surface_expr_in_module(&e, cx)),
            ),
            SurfaceExpr::Case(scr, arms) => TermExpr::Case(
                Box::new(self.resolve_surface_expr_in_module(&scr, cx)),
                arms.iter()
                    .map(|a| self.resolve_surface_case_arm_in_module(a, cx))
                    .collect(),
            ),
            SurfaceExpr::BinaryOperator(op, lhs, rhs) => {
                let core_binop = match op {
                    SurfaceBinOp::Plus => BinOp::Plus,
                    SurfaceBinOp::Minus => BinOp::Minus,
                    SurfaceBinOp::Mult => BinOp::Mult,
                    SurfaceBinOp::Div => BinOp::Div,
                    SurfaceBinOp::Mod => BinOp::Mod,
                    SurfaceBinOp::Less => BinOp::Less,
                    SurfaceBinOp::Greater => BinOp::Greater,
                    SurfaceBinOp::LessOrEqual => BinOp::LessOrEqual,
                    SurfaceBinOp::GreaterOrEqual => BinOp::GreaterOrEqual,
                    SurfaceBinOp::Equal => BinOp::Equal,
                };

                TermExpr::BinaryOperator(
                    core_binop,
                    Box::new(self.resolve_surface_expr_in_module(&lhs, cx)),
                    Box::new(self.resolve_surface_expr_in_module(&rhs, cx)),
                )
            }
            SurfaceExpr::UnaryOperator(op, rhs) => {
                let core_unop = match op {
                    SurfaceUnOp::Neg => UnOp::Neg,
                };
                TermExpr::UnaryOperator(
                    core_unop,
                    Box::new(self.resolve_surface_expr_in_module(&rhs, cx)),
                )
            }
            SurfaceExpr::Let(binder_name, binder_expr, in_expr) => {
                let resolved_binder = self.resolve_surface_expr_in_module(&binder_expr, cx);
                let resolved_in_expr = self.resolve_surface_expr_in_module(&in_expr, cx);

                TermExpr::Let(
                    binder_name.0.to_string(),
                    Box::new(resolved_binder),
                    Box::new(resolved_in_expr),
                )
            }
        };

        TermExprNode::new(node, span)
    }

    fn resolve_surface_case_arm_in_module<'src>(
        &mut self,
        sf: &SpannedWrapper<SurfaceMatchArm<'src>>,
        cx: &ModuleContext,
    ) -> CaseArmNode<Span> {
        CaseArmNode::new(
            CaseArm {
                pattern: self.resolve_surface_pattern_in_module(&sf.0.pattern, cx),
                expr: self.resolve_surface_expr_in_module(&sf.0.expr, cx),
            },
            sf.1.into(),
        )
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

    fn wrap_function_body_without_signature_in_module<'src>(
        &mut self,
        body: &SpannedWrapper<SurfaceExpr<'src>>,
        params: &Vec<SpannedWrapper<&'src str>>,
        cx: &ModuleContext,
    ) -> TermExprNode<Span> {
        let span: Span = body.1.into();
        let mut body_node = self.resolve_surface_expr_in_module(body, cx);

        for p in params.iter().rev() {
            body_node = TermExprNode::new(
                TermExpr::Abs(p.0.to_string(), None, Box::new(body_node)),
                span.clone(),
            );
        }

        body_node
    }

    fn wrap_function_body_with_signature_in_module<'src>(
        &mut self,
        sig: &TypeExpr,
        body: &SpannedWrapper<SurfaceExpr<'src>>,
        params: &Vec<SpannedWrapper<&'src str>>,
        name: &'src str,
        cx: &ModuleContext,
    ) -> TermExprNode<Span> {
        let span: Span = body.1.into();
        let mut body_node = self.resolve_surface_expr_in_module(body, cx);
        let (quants, after) = self.peel_foralls(sig);

        let mut remaining = after;
        let mut annos: Vec<(String, TypeExpr)> = Vec::new();
        for p in params.iter() {
            let (arg_ty, ret_ty) = self.peel_arrow(remaining).unwrap_or_else(|| {
                panic!(
                    "parameter mismatch for '{}'. The number of parameters doesn't match the function signaure",
                    name
                )
            });
            annos.push((p.0.to_string(), arg_ty.clone()));
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

    pub fn resolve_module<'src>(
        mut self,
        module_name: &ModuleName,
        parsed: &ParsedModule<'src>,
        cx: &ModuleContext,
        imported: &ResolverImports,
    ) -> Result<ResolvedModule<Span>, KindError> {
        let mut raw_data_defs = Vec::new();

        for defn in parsed.defs.iter() {
            if let SurfaceDefinition::DataDef {
                name: spanned_name,
                ty_params,
                constructors,
            } = &defn.0
            {
                let (name, span) = spanned_name;
                raw_data_defs.push(RawDataDef {
                    name: qualify(module_name, name),
                    ty_params: ty_params.iter().map(|p| p.0.to_string()).collect(),
                    constructors: constructors
                        .iter()
                        .map(|c| RawConstructorDef {
                            name: qualify(module_name, c.0.name.0),
                            fields: c
                                .0
                                .params
                                .iter()
                                .map(|t| self.resolve_surface_type_in_module(t, cx))
                                .collect(),
                        })
                        .collect(),
                });
            }
        }

        let (inferred_data, mut tycon_kinds) =
            infer_all_data_kinds_with_env(raw_data_defs, imported.tycon_kinds.clone())?;

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

            tycon_kinds.insert(idef.name.clone(), idef.tycon_kind.clone());

            data_defs.push(DataDefinition {
                name: idef.name.clone(),
                ty_params: idef.ty_params.clone(),
                tycon_kind: idef.tycon_kind.clone(),
                constructors: core_ctors,
            });
        }

        let mut func_types: HashMap<String, TypeExpr> = HashMap::new();

        for defn in parsed.defs.iter() {
            if let SurfaceDefinition::SigDef {
                name: spanned_name,
                ty,
            } = &defn.0
            {
                let (name, span) = spanned_name;
                let raw_sig = self.resolve_surface_type_in_module(&ty, cx);

                let mut sig_tycon_env = imported.tycon_kinds.clone();
                sig_tycon_env.extend(tycon_kinds.clone());

                let inf = SignatureKindInferencer::with_tycon_env(sig_tycon_env);
                let info = inf.infer_signature(&raw_sig)?;
                func_types.insert(qualify(module_name, name), info.kinded_signature.clone());
            }
        }

        let mut func_defs = Vec::new();

        for defn in parsed.defs.iter() {
            if let SurfaceDefinition::FunDef {
                name: spanned_name,
                body,
                params,
            } = &defn.0
            {
                let (name, span) = spanned_name;
                let fn_name = qualify(module_name, name);
                let sig = func_types.get(&fn_name).cloned();

                let wrapped = match &sig {
                    Some(sig) => self.wrap_function_body_with_signature_in_module(
                        sig, &body, &params, &fn_name, cx,
                    ),
                    None => self.wrap_function_body_without_signature_in_module(&body, &params, cx),
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
