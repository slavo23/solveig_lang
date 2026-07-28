impl<'a> Resolver<'a> {
    fn builtin_operator_fixity(&self, symbol: Symbol) -> Option<Fixity> {
        match self.symbol_text(symbol).as_str() {
            "::" => Some(Fixity {
                prec: 5,
                assoc: Assoc::Right,
            }),
            _ => None,
        }
    }

    fn resolve_operator_fixity(
        &mut self,
        op: SpannedWrapper<Symbol>,
        cx: &ModuleContext,
    ) -> Result<Fixity, FixityError> {
        if let Some(fixity) = self.builtin_operator_fixity(op.0) {
            return Ok(fixity);
        }

        if cx.local_api.terms.contains_key(&op.0) {
            return Ok(cx
                .local_api
                .fixities
                .get(&op.0)
                .copied()
                .unwrap_or(Fixity::DEFAULT));
        }

        let mut candidates = Vec::new();
        let mut imported_fixity = None;

        for imp in &cx.imported_apis {
            if let Some(def_id) = imp.api.terms.get(&op.0)
                && !candidates.contains(def_id)
            {
                candidates.push(*def_id);
                if imported_fixity.is_none() {
                    imported_fixity = imp.api.fixities.get(&op.0).copied();
                }
            }
        }

        match candidates.as_slice() {
            [_] => Ok(imported_fixity.unwrap_or(Fixity::DEFAULT)),
            [] => {
                self.report_unknown_name(
                    self.file_span(cx, op.1),
                    ResolveNameKind::Operator,
                    self.symbol_text(op.0),
                );
                Err(FixityError::UnknownOperator {
                    span: op.1,
                    op: op.0,
                })
            }
            _ => {
                self.report_ambiguous_import(
                    self.file_span(cx, op.1),
                    ResolveNameKind::Operator,
                    op.0,
                    candidates,
                );
                Err(FixityError::UnknownOperator {
                    span: op.1,
                    op: op.0,
                })
            }
        }
    }

    fn report_fixity_error(&mut self, cx: &ModuleContext, error: &FixityError) {
        match error {
            FixityError::UnknownOperator { .. } => {}
            FixityError::Conflict { span, left, right } => {
                self.errors.push(ResolveError::FixityConflict {
                    span: self.file_span(cx, *span),
                    left: *left,
                    right: *right,
                });
            }
            FixityError::IllegalUnaryMinus { span } => {
                self.errors.push(ResolveError::IllegalUnaryMinus {
                    span: self.file_span(cx, *span),
                });
            }
            FixityError::ExpectedExpression { span } => {
                self.errors.push(ResolveError::MalformedOperatorExpression {
                    span: self.file_span(cx, *span),
                    message: "expected expression in operator sequence".to_string(),
                });
            }
        }
    }

    fn lower_list_cons_application(
        &mut self,
        lhs: &SpannedWrapper<SurfaceExpr>,
        rhs: &SpannedWrapper<SurfaceExpr>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
        bound_terms: &FxHashSet<Symbol>,
        span: Span,
    ) -> TermExpr<Span> {
        let cons = TermExprNode::new(
            TermExpr::Con(prelude_list_cons_ctor_id(self.names)),
            span.clone(),
        );
        let lhs = self.resolve_surface_expr_in_module(lhs, cx, local, bound_terms);
        let rhs = self.resolve_surface_expr_in_module(rhs, cx, local, bound_terms);
        let app = TermExprNode::new(
            TermExpr::App(Box::new(cons), Box::new(lhs)),
            span,
        );

        TermExpr::App(Box::new(app), Box::new(rhs))
    }

    fn lower_operator_application(
        &mut self,
        lhs: &SpannedWrapper<SurfaceExpr>,
        op: SpannedWrapper<Symbol>,
        rhs: &SpannedWrapper<SurfaceExpr>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
        bound_terms: &FxHashSet<Symbol>,
    ) -> TermExpr<Span> {
        let span = to_core_span(SourceSpan::new(lhs.1.start, rhs.1.end));

        if self.symbol_text(op.0) == "::" {
            return self.lower_list_cons_application(lhs, rhs, cx, local, bound_terms, span);
        }

        let name_ref = NameRef {
            segments: vec![op.0],
        };
        let op_node = TermExprNode::new(
            TermExpr::Var(self.resolve_term_ref_with_binders(
                &name_ref,
                self.file_span(cx, op.1),
                cx,
                bound_terms,
            )),
            to_core_span(op.1),
        );
        let lhs = self.resolve_surface_expr_in_module(lhs, cx, local, bound_terms);
        let rhs = self.resolve_surface_expr_in_module(rhs, cx, local, bound_terms);
        let app = TermExprNode::new(
            TermExpr::App(Box::new(op_node), Box::new(lhs)),
            span,
        );

        TermExpr::App(Box::new(app), Box::new(rhs))
    }

    fn section_param_expr(
        &mut self,
        source_span: SourceSpan,
        cst_id: solveig_syntax::cst::CstId,
    ) -> (Symbol, SpannedWrapper<SurfaceExpr>) {
        let symbol = self.session.intern("$section");
        self.names.register_symbol(symbol, "$section");
        (
            symbol,
            (
                SurfaceExpr::Var((
                    NameRef {
                        segments: vec![symbol],
                    },
                    source_span,
                    cst_id,
                )),
                source_span,
                cst_id,
            ),
        )
    }

    fn lower_operator_section(
        &mut self,
        lhs: &SpannedWrapper<SurfaceExpr>,
        op: SpannedWrapper<Symbol>,
        rhs: &SpannedWrapper<SurfaceExpr>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
        bound_terms: &FxHashSet<Symbol>,
    ) -> TermExpr<Span> {
        if let Err(error) = self.resolve_operator_fixity(op, cx) {
            self.report_fixity_error(cx, &error);
            return TermExpr::Hole;
        }

        let mut section_bound_terms = bound_terms.clone();
        section_bound_terms.insert(self.session.intern("$section"));
        let body = self.lower_operator_application(lhs, op, rhs, cx, local, &section_bound_terms);
        let span = to_core_span(SourceSpan::new(lhs.1.start, rhs.1.end));
        let body = TermExprNode::new(body, span);
        TermExpr::Abs("$section".to_string(), None, Box::new(body))
    }

    fn prelude_typeclasses_var(
        &self,
        name: &'static str,
        span: SourceSpan,
        cst_id: solveig_syntax::cst::CstId,
    ) -> SpannedWrapper<SurfaceExpr> {
        (
            SurfaceExpr::Var((
                NameRef {
                    segments: vec![
                        self.session.intern("prelude"),
                        self.session.intern("typeclasses"),
                        self.session.intern(name),
                    ],
                },
                span,
                cst_id,
            )),
            span,
            cst_id,
        )
    }

    fn surface_app(
        &self,
        func: SpannedWrapper<SurfaceExpr>,
        arg: SpannedWrapper<SurfaceExpr>,
    ) -> SpannedWrapper<SurfaceExpr> {
        let span = SourceSpan::new(func.1.start, arg.1.end);
        let cst_id = func.2;
        (SurfaceExpr::App(Box::new(func), Box::new(arg)), span, cst_id)
    }

    fn surface_call2(
        &self,
        name: &'static str,
        first: SpannedWrapper<SurfaceExpr>,
        second: SpannedWrapper<SurfaceExpr>,
        span: SourceSpan,
        cst_id: solveig_syntax::cst::CstId,
    ) -> SpannedWrapper<SurfaceExpr> {
        let callee = self.prelude_typeclasses_var(name, span, cst_id);
        let applied = self.surface_app(callee, first);
        self.surface_app(applied, second)
    }

    // Haskell 2010 Report §3.14, do expressions; Solveig keeps the same
    // nested bind/then shape while resolving operators through Prelude names.
    fn desugar_do_statements(
        &self,
        statements: &[SpannedWrapper<SurfaceDoStmt>],
    ) -> SpannedWrapper<SurfaceExpr> {
        let (last, prefix) = statements
            .split_last()
            .expect("ICE: parser must reject empty do blocks before resolution");

        let mut body = match &last.0 {
            SurfaceDoStmt::Expr(expr) => expr.clone(),
            SurfaceDoStmt::Bind { .. } | SurfaceDoStmt::Let { .. } => {
                (SurfaceExpr::Hole, last.1, last.2)
            }
        };

        for statement in prefix.iter().rev() {
            body = match &statement.0 {
                SurfaceDoStmt::Expr(action) => {
                    self.surface_call2("thenM", action.clone(), body, statement.1, statement.2)
                }
                SurfaceDoStmt::Bind { binder, expr } => {
                    if self.symbol_text(binder.0) == "_" {
                        self.surface_call2("thenM", expr.clone(), body, statement.1, statement.2)
                    } else {
                        let lambda_span = SourceSpan::new(binder.1.start, body.1.end);
                        let lambda = (
                            SurfaceExpr::Abs(vec![(*binder, None)], Box::new(body)),
                            lambda_span,
                            statement.2,
                        );
                        self.surface_call2("bind", expr.clone(), lambda, statement.1, statement.2)
                    }
                }
                SurfaceDoStmt::Let { binder, expr } => {
                    let span = SourceSpan::new(statement.1.start, body.1.end);
                    (
                        SurfaceExpr::Let(*binder, Box::new(expr.clone()), Box::new(body)),
                        span,
                        statement.2,
                    )
                }
            };
        }

        body
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
                            self.symbol_text(*name),
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
        bound_terms: &FxHashSet<Symbol>,
    ) -> VarRef {
        match r.segments.as_slice() {
            [name] if bound_terms.contains(name) => {
                let name_text = self.symbol_text(*name);
                self.names.register_symbol(*name, &name_text);
                VarRef::Local(*name)
            }
            _ => match self.resolve_term_defid(r, span, cx) {
                Some(def_id) => {
                    self.record_resolution(span, def_id);
                    self.qualified_def_name(def_id);
                    VarRef::Global(def_id)
                }
                None => {
                    let unresolved = self.unresolved_name(r);
                    VarRef::named_global(self.names, &unresolved)
                }
            },
        }
    }
    fn resolve_surface_expr_in_module(
        &mut self,
        sf: &SpannedWrapper<SurfaceExpr>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
        bound_terms: &FxHashSet<Symbol>,
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
            SurfaceExpr::Con(con_name) => TermExpr::Con(self.resolve_ctor_id(
                &con_name.0,
                self.file_span(cx, con_name.1),
                cx,
            )),

            SurfaceExpr::Literal(lit) => TermExpr::Lit(self.resolve_literal(lit)),

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
                            self.symbol_text(param_name.0),
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

            SurfaceExpr::Do(statements) => {
                let expr = self.desugar_do_statements(statements);
                return self.resolve_surface_expr_in_module(&expr, cx, local, bound_terms);
            }

            SurfaceExpr::UnaryOperator(op, rhs) => {
                let name_ref = match op {
                    SurfaceUnOp::Neg => NameRef {
                        segments: vec![self.session.intern("negate")],
                    },
                };
                let op_node = TermExprNode::new(
                    TermExpr::Var(self.resolve_term_ref_with_binders(
                        &name_ref,
                        self.file_span(cx, sf.1),
                        cx,
                        bound_terms,
                    )),
                    span.clone(),
                );
                let rhs = self.resolve_surface_expr_in_module(rhs, cx, local, bound_terms);
                TermExpr::App(Box::new(op_node), Box::new(rhs))
            }

            SurfaceExpr::Let(binder_name, binder_expr, in_expr) => {
                let resolved_binder =
                    self.resolve_surface_expr_in_module(binder_expr, cx, local, bound_terms);

                let mut body_bound_terms = bound_terms.clone();
                body_bound_terms.insert(binder_name.0);

                let resolved_in_expr =
                    self.resolve_surface_expr_in_module(in_expr, cx, local, &body_bound_terms);

                TermExpr::Let(
                    self.symbol_text(binder_name.0),
                    Box::new(resolved_binder),
                    Box::new(resolved_in_expr),
                )
            }

            SurfaceExpr::OperatorApply(lhs, op, rhs) => self.lower_operator_application(
                lhs,
                *op,
                rhs,
                cx,
                local,
                bound_terms,
            ),

            SurfaceExpr::OperatorLeftSection(lhs, op) => {
                let (_, rhs) = self.section_param_expr(sf.1, sf.2);
                self.lower_operator_section(lhs, *op, &rhs, cx, local, bound_terms)
            }

            SurfaceExpr::OperatorRightSection(op, rhs) => {
                let (_, lhs) = self.section_param_expr(sf.1, sf.2);
                self.lower_operator_section(&lhs, *op, rhs, cx, local, bound_terms)
            }

            SurfaceExpr::OpSeq(first, segments) => {
                match resolve_op_seq(first, segments, |op| self.resolve_operator_fixity(op, cx)) {
                    Ok(expr) => {
                        return self.resolve_surface_expr_in_module(&expr, cx, local, bound_terms);
                    }
                    Err(error) => {
                        self.report_fixity_error(cx, &error);
                        TermExpr::Hole
                    }
                }
            }
        };

        TermExprNode::new(node, span)
    }
    fn peel_arrow<'ty>(&self, ty: &'ty Ty) -> Option<(&'ty Ty, &'ty Ty)> {
        match ty {
            Ty::TyArr(lhs, rhs) => Some((lhs, rhs)),
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
        let mut bound_terms = FxHashSet::default();

        for param in params {
            bound_terms.insert(param.0);
        }

        let mut body_node = self.resolve_surface_expr_in_module(body, cx, local, &bound_terms);

        for param in params.iter().rev() {
            body_node = TermExprNode::new(
                TermExpr::Abs(
                    self.symbol_text(param.0),
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
        let mut bound_terms = FxHashSet::default();

        for param in params {
            bound_terms.insert(param.0);
        }

        let mut body_node = self.resolve_surface_expr_in_module(body, cx, local, &bound_terms);

        let mut remaining = &sig.ty;
        let mut annos: Vec<(String, Ty)> = Vec::new();

        for param in params {
            let Some((arg_ty, ret_ty)) = self.peel_arrow(remaining) else {
                self.errors.push(ResolveError::SignatureArity {
                    span: self.file_span(cx, name.1),
                    name: name.0,
                    params: params.len(),
                });
                break;
            };

            annos.push((self.symbol_text(param.0), arg_ty.clone()));
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
                TermExpr::TyAbs(
                    self.names.tyvar_display_name(quant_name),
                    quant_kind,
                    Box::new(body_node),
                ),
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
        func_schemes: &FxHashMap<DefId, Scheme>,
    ) -> Vec<TermDefinition<Span>> {
        parsed
            .defs
            .iter()
            .filter_map(|defn| match &defn.0 {
                SurfaceDefinition::FunDef {
                    name: spanned_name,
                    body,
                    params,
                    ..
                } => {
                    let (name, _span, _) = spanned_name;
                    let fn_id = self.local_def_id(cx, *name);
                    let fn_name = self.qualified_def_name(fn_id);
                    let sig = func_schemes.get(&fn_id).cloned();

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
                        id: fn_id,
                        name: fn_name,
                        short_name: self.symbol_text(*name),
                        signature: sig,
                        body: wrapped,
                    })
                }

                _ => None,
            })
            .collect()
    }
}
