fn elab_node(
    node: TermExpr<TcMetadata, Span>,
    span: Span,
    ty: TypeExpr,
) -> TermExprNode<TcMetadata, Span> {
    TermExprNode::new(node, TcMetadata::new(span, ty))
}

impl TypeChecker {
    fn zonk_scheme(&self, scheme: &Scheme) -> Scheme {
        Scheme {
            quantifiers: scheme.quantifiers.clone(),
            predicates: scheme
                .predicates
                .iter()
                .map(|pred| solveig_ast::ast::Predicate {
                    class_name: pred.class_name.clone(),
                    args: pred.args.iter().map(|arg| self.zonk_type(arg)).collect(),
                })
                .collect(),
            ty: self.zonk_type(&scheme.ty),
        }
    }
    fn zonk_predicate(&self, pred: &Predicate) -> Predicate {
        Predicate {
            class_name: pred.class_name.clone(),
            args: pred.args.iter().map(|arg| self.zonk_type(arg)).collect(),
        }
    }
    fn record_solution(&mut self, solved_entry: &ContextEntry) {
        if let ContextEntry::SolvedEVar {
            lhs: evar_id,
            rhs: solution,
        } = solved_entry
        {
            self.evar_solutions
                .insert(*evar_id, solution.clone());
        }
    }
    fn context_bindings(&self, ctx: &Context) -> Vec<(String, TypeExpr)> {
        ctx.get_entries()
            .iter()
            .filter_map(|entry| {
                if let ContextEntry::TypedVar { var_name, ty } = entry {
                    Some((var_name.clone(), self.zonk_type(&ctx.apply(ty))))
                } else {
                    None
                }
            })
            .collect()
    }
    fn report_hole(&mut self, ctx: &Context, span: Span, ty: &TypeExpr) {
        self.holes.push(HoleInfo {
            span: self.get_file_span(span),
            expected_type: self.zonk_type(&ctx.apply(ty)),
            context: self.context_bindings(ctx),
        });
    }
    pub fn finalize_holes(&mut self) {
        let mut holes = std::mem::take(&mut self.holes);
        for hole in &mut holes {
            hole.expected_type = self.zonk_type(&hole.expected_type);
            for (_, ty) in &mut hole.context {
                *ty = self.zonk_type(ty);
            }
        }
        self.holes = holes;
    }
    pub fn zonk_type(&self, ty: &TypeExpr) -> TypeExpr {
        match ty {
            TypeExpr::TyExVar(id) => {
                if let Some(solution) = self.evar_solutions.get(id) {
                    self.zonk_type(solution)
                } else {
                    ty.clone()
                }
            }
            TypeExpr::TyArr(lhs, rhs) => {
                TypeExpr::make_arr(self.zonk_type(lhs), self.zonk_type(rhs))
            }
            TypeExpr::TyApp(lhs, rhs) => {
                TypeExpr::make_app(self.zonk_type(lhs), self.zonk_type(rhs))
            }
            TypeExpr::TyForAll(quantifier, kind, body) => {
                TypeExpr::make_forall_id(*quantifier, kind.clone(), self.zonk_type(body))
            }
            TypeExpr::Qualified(preds, body) => TypeExpr::make_qualified(
                preds
                    .iter()
                    .map(|pred| solveig_ast::ast::Predicate {
                        class_name: pred.class_name.clone(),
                        args: pred.args.iter().map(|arg| self.zonk_type(arg)).collect(),
                    })
                    .collect(),
                self.zonk_type(body),
            ),
            _ => ty.clone(),
        }
    }
    pub fn zonk_expr(
        &self,
        expr: &TermExprNode<TcMetadata, Span>,
    ) -> TermExprNode<TcMetadata, Span> {
        let zonked_meta = TcMetadata::new(
            expr.anno.span.clone(),
            self.zonk_type(&expr.anno.inferred_type),
        );
        let zonked_node = match &expr.node {
            TermExpr::Hole => TermExpr::Hole,
            TermExpr::Unit => TermExpr::Unit,
            TermExpr::Var(name) => TermExpr::Var(name.clone()),
            TermExpr::Lit(lit_val) => TermExpr::Lit(lit_val.clone()),
            TermExpr::Con(id) => TermExpr::Con(*id),
            TermExpr::App(func, arg) => TermExpr::App(
                Box::new(self.zonk_expr(func)),
                Box::new(self.zonk_expr(arg)),
            ),
            TermExpr::Abs(param, annotation, body) => TermExpr::Abs(
                param.clone(),
                annotation.as_ref().map(|ty| self.zonk_type(ty)),
                Box::new(self.zonk_expr(body)),
            ),
            TermExpr::TyAbs(param, kind, body) => {
                TermExpr::TyAbs(param.clone(), kind.clone(), Box::new(self.zonk_expr(body)))
            }
            TermExpr::TyApp(inner, ty_arg) => {
                TermExpr::TyApp(Box::new(self.zonk_expr(inner)), self.zonk_type(ty_arg))
            }
            TermExpr::Anno(inner, ty) => {
                TermExpr::Anno(Box::new(self.zonk_expr(inner)), self.zonk_type(ty))
            }
            TermExpr::Case(scrutinee, arms) => TermExpr::Case(
                Box::new(self.zonk_expr(scrutinee)),
                arms.iter()
                    .map(|arm| {
                        CaseArmNode::new(
                            CaseArm {
                                pattern: arm.node.pattern.clone(),
                                expr: self.zonk_expr(&arm.node.expr),
                            },
                            arm.anno.clone(),
                        )
                    })
                    .collect(),
            ),
            TermExpr::IfThenElse(cond, then_branch, else_branch) => TermExpr::IfThenElse(
                Box::new(self.zonk_expr(cond)),
                Box::new(self.zonk_expr(then_branch)),
                Box::new(self.zonk_expr(else_branch)),
            ),
            TermExpr::BinaryOperator(op, lhs, rhs) => TermExpr::BinaryOperator(
                *op,
                Box::new(self.zonk_expr(lhs)),
                Box::new(self.zonk_expr(rhs)),
            ),
            TermExpr::UnaryOperator(op, rhs) => {
                TermExpr::UnaryOperator(*op, Box::new(self.zonk_expr(rhs)))
            }
            TermExpr::Let(binder_name, binder_expr, in_expr) => TermExpr::Let(
                binder_name.clone(),
                Box::new(self.zonk_expr(binder_expr)),
                Box::new(self.zonk_expr(in_expr)),
            ),
        };
        TermExprNode::new(zonked_node, zonked_meta)
    }
}
