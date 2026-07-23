impl<'a> Resolver<'a> {
    fn resolve_term_ref(&mut self, r: &NameRef, span: FileSpan, cx: &ModuleContext) -> String {
        match self.resolve_term_defid(r, span, cx) {
            Some(def_id) => {
                self.record_resolution(span, def_id);
                self.legacy_name(def_id)
            }
            None => self.unresolved_name(r),
        }
    }
    fn resolve_term_defid(
        &mut self,
        r: &NameRef,
        span: FileSpan,
        cx: &ModuleContext,
    ) -> Option<DefId> {
        match r.segments.as_slice() {
            [name] => {
                if let Some(sym) = cx.builtins.terms.get(name) {
                    return Some(*sym);
                }

                if let Some(sym) = cx.local_api.terms.get(name) {
                    return Some(*sym);
                }

                let mut candidates = Vec::new();

                for imp in &cx.imported_apis {
                    if let Some(sym) = imp.api.terms.get(name)
                        && !candidates.contains(sym)
                    {
                        candidates.push(*sym);
                    }
                }

                match candidates.as_slice() {
                    [sym] => Some(*sym),
                    [] => {
                        self.report_unknown_name(
                            span,
                            ResolveNameKind::Term,
                            self.symbol_text(*name).to_string(),
                        );
                        None
                    }
                    _ => {
                        self.report_ambiguous_import(
                            span,
                            ResolveNameKind::Term,
                            *name,
                            candidates,
                        );
                        None
                    }
                }
            }

            [_first, rest @ ..] => {
                let local_name = *rest
                    .last()
                    .expect("ICE: qualified term name must have a last segment");

                if cx.module.0.as_slice() == r.prefix() {
                    if let Some(sym) = cx.local_api.terms.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(span, ResolveNameKind::Term, self.unresolved_name(r));
                    return None;
                }

                if let Some(imp) = self.imported_by_alias_or_module(r, cx) {
                    if let Some(sym) = imp.api.terms.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(span, ResolveNameKind::Term, self.unresolved_name(r));
                    return None;
                }

                self.report_unknown_module(span, ModuleName(r.prefix().to_vec()));
                None
            }

            [] => panic!("ICE: empty term name ref"),
        }
    }
    fn resolve_term_ref_with_binders(
        &mut self,
        r: &NameRef,
        span: FileSpan,
        cx: &ModuleContext,
        bound_terms: &HashSet<Symbol>,
    ) -> String {
        match r.segments.as_slice() {
            [name] if bound_terms.contains(name) => self.symbol_text(*name).to_string(),
            _ => self.resolve_term_ref(r, span, cx),
        }
    }
    fn resolve_surface_expr_in_module(
        &mut self,
        sf: &SpannedWrapper<SurfaceExpr>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
        bound_terms: &HashSet<Symbol>,
    ) -> TermExprNode<Span> {
        let span = to_core_span(sf.1);

        let node = match &sf.0 {
            SurfaceExpr::Unit => TermExpr::Unit,
            SurfaceExpr::Hole => TermExpr::Hole,
            SurfaceExpr::Var(var_name) => TermExpr::Var(self.resolve_term_ref_with_binders(
                &var_name.0,
                self.file_span(cx, var_name.1),
                cx,
                bound_terms,
            )),
            SurfaceExpr::Con(con_name) => TermExpr::Con(self.resolve_ctor_ref(
                &con_name.0,
                self.file_span(cx, con_name.1),
                cx,
            )),

            SurfaceExpr::Literal(lit) => match lit {
                SurfaceLiteral::Int(value) => TermExpr::Lit(Literal::Int(*value)),
            },

            SurfaceExpr::App(lhs, rhs) => TermExpr::App(
                Box::new(self.resolve_surface_expr_in_module(lhs, cx, local, bound_terms)),
                Box::new(self.resolve_surface_expr_in_module(rhs, cx, local, bound_terms)),
            ),

            SurfaceExpr::Ann(expr, ty) => TermExpr::Anno(
                Box::new(self.resolve_surface_expr_in_module(expr, cx, local, bound_terms)),
                self.resolve_surface_type_in_module(ty, cx, local),
            ),

            SurfaceExpr::Abs(params, body) => {
                let mut body_bound_terms = bound_terms.clone();

                for (param_name, _) in params {
                    body_bound_terms.insert(param_name.0);
                }

                let mut body_node =
                    self.resolve_surface_expr_in_module(body, cx, local, &body_bound_terms);

                for (param_name, param_ty_anno) in params.iter().rev() {
                    let param_ty = param_ty_anno
                        .as_ref()
                        .map(|ty| self.resolve_surface_type_in_module(ty, cx, local));

                    body_node = TermExprNode::new(
                        TermExpr::Abs(
                            self.symbol_text(param_name.0).to_string(),
                            param_ty,
                            Box::new(body_node),
                        ),
                        span.clone(),
                    );
                }

                return body_node;
            }

            SurfaceExpr::IfThenElse(cond, then_branch, else_branch) => TermExpr::IfThenElse(
                Box::new(self.resolve_surface_expr_in_module(cond, cx, local, bound_terms)),
                Box::new(self.resolve_surface_expr_in_module(then_branch, cx, local, bound_terms)),
                Box::new(self.resolve_surface_expr_in_module(else_branch, cx, local, bound_terms)),
            ),

            SurfaceExpr::Case(scrutinee, arms) => TermExpr::Case(
                Box::new(self.resolve_surface_expr_in_module(scrutinee, cx, local, bound_terms)),
                arms.iter()
                    .map(|arm| self.resolve_surface_case_arm_in_module(arm, cx, local, bound_terms))
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
                    Box::new(self.resolve_surface_expr_in_module(lhs, cx, local, bound_terms)),
                    Box::new(self.resolve_surface_expr_in_module(rhs, cx, local, bound_terms)),
                )
            }

            SurfaceExpr::UnaryOperator(op, rhs) => {
                let core_unop = match op {
                    SurfaceUnOp::Neg => UnOp::Neg,
                };

                TermExpr::UnaryOperator(
                    core_unop,
                    Box::new(self.resolve_surface_expr_in_module(rhs, cx, local, bound_terms)),
                )
            }

            SurfaceExpr::Let(binder_name, binder_expr, in_expr) => {
                let resolved_binder =
                    self.resolve_surface_expr_in_module(binder_expr, cx, local, bound_terms);

                let mut body_bound_terms = bound_terms.clone();
                body_bound_terms.insert(binder_name.0);

                let resolved_in_expr =
                    self.resolve_surface_expr_in_module(in_expr, cx, local, &body_bound_terms);

                TermExpr::Let(
                    self.symbol_text(binder_name.0).to_string(),
                    Box::new(resolved_binder),
                    Box::new(resolved_in_expr),
                )
            }
        };

        TermExprNode::new(node, span)
    }
    fn peel_arrow<'ty>(&self, ty: &'ty TypeExpr) -> Option<(&'ty TypeExpr, &'ty TypeExpr)> {
        match ty {
            TypeExpr::TyArr(lhs, rhs) => Some((lhs, rhs)),
            _ => None,
        }
    }
    fn wrap_function_body_without_signature_in_module(
        &mut self,
        body: &SpannedWrapper<SurfaceExpr>,
        params: &[SpannedWrapper<Symbol>],
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> TermExprNode<Span> {
        let span = to_core_span(body.1);
        let mut bound_terms = HashSet::new();

        for param in params {
            bound_terms.insert(param.0);
        }

        let mut body_node = self.resolve_surface_expr_in_module(body, cx, local, &bound_terms);

        for param in params.iter().rev() {
            body_node = TermExprNode::new(
                TermExpr::Abs(
                    self.symbol_text(param.0).to_string(),
                    None,
                    Box::new(body_node),
                ),
                span.clone(),
            );
        }

        body_node
    }
    fn wrap_function_body_with_signature_in_module(
        &mut self,
        sig: &Scheme,
        body: &SpannedWrapper<SurfaceExpr>,
        params: &[SpannedWrapper<Symbol>],
        name: SpannedWrapper<Symbol>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> TermExprNode<Span> {
        let span = to_core_span(body.1);
        let mut bound_terms = HashSet::new();

        for param in params {
            bound_terms.insert(param.0);
        }

        let mut body_node = self.resolve_surface_expr_in_module(body, cx, local, &bound_terms);

        let mut remaining = &sig.ty;
        let mut annos: Vec<(String, TypeExpr)> = Vec::new();

        for param in params {
            let Some((arg_ty, ret_ty)) = self.peel_arrow(remaining) else {
                self.errors.push(ResolveError::SignatureArity {
                    span: self.file_span(cx, name.1),
                    name: name.0,
                    params: params.len(),
                });
                break;
            };

            annos.push((self.symbol_text(param.0).to_string(), arg_ty.clone()));
            remaining = ret_ty;
        }

        for (param_name, param_ty) in annos.into_iter().rev() {
            body_node = TermExprNode::new(
                TermExpr::Abs(param_name, Some(param_ty), Box::new(body_node)),
                span.clone(),
            );
        }

        for (quant_name, quant_kind) in sig.quantifiers.clone().into_iter().rev() {
            body_node = TermExprNode::new(
                TermExpr::TyAbs(tyvar_display_name(quant_name), quant_kind, Box::new(body_node)),
                span.clone(),
            );
        }

        body_node
    }
    fn resolve_function_defs(
        &mut self,
        _module_name: &ModuleName,
        parsed: &ParsedModule,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
        func_schemes: &HashMap<String, Scheme>,
    ) -> Vec<TermDefinition<Span>> {
        parsed
            .defs
            .iter()
            .filter_map(|defn| match &defn.0 {
                SurfaceDefinition::FunDef {
                    name: spanned_name,
                    body,
                    params,
                } => {
                    let (name, _span) = spanned_name;
                    let fn_name = self.legacy_name(self.local_def_id(cx, *name));
                    let sig = func_schemes.get(&fn_name).cloned();

                    let wrapped = match &sig {
                        Some(sig) => self.wrap_function_body_with_signature_in_module(
                            sig,
                            body,
                            params,
                            *spanned_name,
                            cx,
                            local,
                        ),
                        None => self.wrap_function_body_without_signature_in_module(
                            body, params, cx, local,
                        ),
                    };

                    Some(TermDefinition {
                        name: fn_name,
                        signature: sig,
                        body: wrapped,
                    })
                }

                _ => None,
            })
            .collect()
    }
}
