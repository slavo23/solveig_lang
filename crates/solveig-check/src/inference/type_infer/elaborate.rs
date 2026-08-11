fn elab_node(
    node: TermExpr<TcMetadata, Span>,
    span: Span,
    ty: Ty,
) -> TermExprNode<TcMetadata, Span> {
    TermExprNode::new(node, TcMetadata::new(span, ty))
}

// Following Wadler-Blott dictionary translation and the typechecker-zoo
// type-classes elaborator, scheme instantiation applies type arguments first
// and then one evidence argument per predicate. C2 records placeholders here;
// `zonk_ev` replaces them with the C1 solver's concrete evidence after
// constraint solving.
fn wrap_ev_apps(
    mut expr: TermExprNode<TcMetadata, Span>,
    evvars: Vec<EvVarId>,
    span: Span,
    ty: &Ty,
) -> TermExprNode<TcMetadata, Span> {
    for ev in evvars {
        expr = elab_node(
            TermExpr::EvApp(Box::new(expr), Ev::Placeholder(ev)),
            span.clone(),
            ty.clone(),
        );
    }
    expr
}

// A qualified scheme `forall a. P => t` elaborates as type lambdas outside
// evidence lambdas: TyAbs(... EvAbs(d : P, body) ...). The recursive checking
// path creates the TyAbs nodes while this helper binds the predicate givens.
fn wrap_ev_abs(
    mut body: TermExprNode<TcMetadata, Span>,
    givens: Vec<GivenEvidence>,
    span: Span,
    ty: &Ty,
) -> TermExprNode<TcMetadata, Span> {
    for given in givens.into_iter().rev() {
        body = elab_node(
            TermExpr::EvAbs(given.ev, given.pred, Box::new(body)),
            span.clone(),
            ty.clone(),
        );
    }
    body
}

impl TypeChecker<'_> {
    fn zonk_scheme(&self, scheme: &Scheme) -> Scheme {
        Scheme {
            quantifiers: scheme.quantifiers.clone(),
            predicates: scheme
                .predicates
                .iter()
                .map(|pred| solveig_ast::ast::Predicate {
                    class_id: pred.class_id,
                    args: pred.args.iter().map(|arg| self.zonk_type(arg)).collect(),
                })
                .collect(),
            ty: self.zonk_type(&scheme.ty),
        }
    }
    fn zonk_predicate(&self, pred: &Predicate) -> Predicate {
        Predicate {
            class_id: pred.class_id,
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
    fn context_bindings(&self, ctx: &Context) -> Vec<(String, Ty)> {
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
    fn report_hole(&mut self, ctx: &Context, span: Span, ty: &Ty) {
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
    pub fn zonk_type(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::TyExVar(id) => {
                if let Some(solution) = self.evar_solutions.get(id) {
                    self.zonk_type(solution)
                } else {
                    ty.clone()
                }
            }
            Ty::TyArr(lhs, rhs) => {
                Ty::make_arr(self.zonk_type(lhs), self.zonk_type(rhs))
            }
            Ty::TyApp(lhs, rhs) => {
                Ty::make_app(self.zonk_type(lhs), self.zonk_type(rhs))
            }
            Ty::TyForAll(binders, preds, body) => Ty::make_forall_group(
                binders.clone(),
                preds
                    .iter()
                    .map(|pred| solveig_ast::ast::Predicate {
                        class_id: pred.class_id,
                        args: pred.args.iter().map(|arg| self.zonk_type(arg)).collect(),
                    })
                    .collect(),
                self.zonk_type(body),
            ),
            _ => ty.clone(),
        }
    }
    fn zonk_ev(&self, ev: &Ev) -> Ev {
        match ev {
            Ev::Var(evvar) => Ev::Var(*evvar),
            Ev::Placeholder(evvar) => {
                let solved = self.evidence_solutions.get(evvar).unwrap_or_else(|| {
                    solveig_base::ice!(
                        "unsolved evidence placeholder {evvar:?} survived constraint solving"
                    )
                });
                self.zonk_ev(solved)
            }
            Ev::Inst(instance, ty_args, evidence_args) => Ev::Inst(
                *instance,
                ty_args.iter().map(|ty| self.zonk_type(ty)).collect(),
                evidence_args.iter().map(|ev| self.zonk_ev(ev)).collect(),
            ),
            Ev::Super(root, class, index) => {
                Ev::Super(Box::new(self.zonk_ev(root)), *class, *index)
            }
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
            TermExpr::EvAbs(ev, pred, body) => TermExpr::EvAbs(
                *ev,
                self.zonk_predicate(pred),
                Box::new(self.zonk_expr(body)),
            ),
            TermExpr::EvApp(inner, ev) => {
                TermExpr::EvApp(Box::new(self.zonk_expr(inner)), self.zonk_ev(ev))
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
            TermExpr::Match(scrutinees, arms) => TermExpr::Match(
                scrutinees
                    .iter()
                    .map(|scrutinee| self.zonk_expr(scrutinee))
                    .collect(),
                arms.iter()
                    .map(|arm| {
                        MatchArmNode::new(
                            MatchArm {
                                patterns: arm.node.patterns.clone(),
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
            TermExpr::Let(binder_name, binder_expr, in_expr) => TermExpr::Let(
                binder_name.clone(),
                Box::new(self.zonk_expr(binder_expr)),
                Box::new(self.zonk_expr(in_expr)),
            ),
        };
        TermExprNode::new(zonked_node, zonked_meta)
    }
}
