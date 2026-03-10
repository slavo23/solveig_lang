use crate::context::{Context, ContextEntry};
use core::{
    core_expr::{CoreCaseArm, CoreExpr, CoreModule, CorePattern},
    core_type::Ty,
};
use std::collections::HashMap;

fn ty_well_formed(ty: &Ty, ctx: &Context) -> bool {
    match ty {
        Ty::TyUnit => true,
        Ty::TyVar(var_name) => ctx.has_var(var_name.clone()),
        Ty::TyArr(lhs, rhs) | Ty::TyApp(lhs, rhs) => {
            ty_well_formed(lhs, ctx) && ty_well_formed(rhs, ctx)
        }

        Ty::TyExVar(evar_name) => {
            ctx.has_evar(evar_name.clone()) || ctx.has_solved_evar(evar_name.clone())
        }
        Ty::TyForAll(quant, body) => ty_well_formed(
            body,
            &ctx.clone().add_entry(ContextEntry::MonoVar(quant.clone())),
        ),
        _ => true,
    }
}

pub struct TypeInference {
    var_counter: usize,
    data_ctors: HashMap<String, Ty>,
    func_types: HashMap<String, Ty>,
    evar_solutions: HashMap<String, Ty>,
    rule_trace: Vec<&'static str>,
}

impl TypeInference {
    pub fn new() -> TypeInference {
        Self {
            var_counter: 0,
            data_ctors: Default::default(),
            func_types: Default::default(),
            evar_solutions: Default::default(),
            rule_trace: Default::default(),
        }
    }

    pub fn with_module_context(
        data_ctors: HashMap<String, Ty>,
        func_types: HashMap<String, Ty>,
    ) -> TypeInference {
        Self {
            var_counter: 0,
            data_ctors,
            func_types,
            evar_solutions: HashMap::new(),
            rule_trace: Vec::new(),
        }
    }

    fn get_data_ctor_ty(&self, name: String) -> Option<Ty> {
        self.data_ctors.get(&name).cloned()
    }

    fn get_func_type(&self, name: String) -> Option<Ty> {
        self.func_types.get(&name).cloned()
    }

    fn increment_counter(&mut self) -> usize {
        self.var_counter += 1;
        self.var_counter
    }

    fn make_fresh_evar(&mut self) -> String {
        format!("a^{}", self.increment_counter())
    }

    fn trace_rule(&mut self, rule_name: &'static str) {
        self.rule_trace.push(rule_name);
    }

    fn record_solution(&mut self, equation: &ContextEntry) {
        match equation {
            ContextEntry::SolvedEVar { lhs, rhs } => {
                self.evar_solutions.insert(lhs.clone(), rhs.clone());
            }
            _ => {}
        }
    }

    fn zonk_ty(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::TyExVar(name) => {
                if let Some(solution) = self.evar_solutions.get(name) {
                    self.zonk_ty(solution)
                } else {
                    ty.clone()
                }
            }
            Ty::TyArr(a, b) => Ty::make_arr(self.zonk_ty(a), self.zonk_ty(b)),
            Ty::TyApp(a, b) => Ty::make_app(self.zonk_ty(a), self.zonk_ty(b)),
            Ty::TyForAll(q, body) => Ty::make_forall(q.clone(), self.zonk_ty(body)),
            _ => ty.clone(),
        }
    }

    fn zonk_expr(&self, expr: &CoreExpr) -> CoreExpr {
        match expr {
            CoreExpr::Unit => expr.clone(),
            CoreExpr::Var(_) | CoreExpr::Con(_) => expr.clone(),
            CoreExpr::TyApp(e, ty) => {
                CoreExpr::TyApp(Box::new(self.zonk_expr(e)), self.zonk_ty(ty))
            }
            CoreExpr::Anno(e, ty) => CoreExpr::anno(self.zonk_expr(e), self.zonk_ty(ty)),
            CoreExpr::App(lhs, rhs) => CoreExpr::app(self.zonk_expr(lhs), self.zonk_expr(rhs)),
            CoreExpr::Abs(arg_name, None, body) => {
                CoreExpr::abs(arg_name.clone(), self.zonk_expr(body))
            }
            CoreExpr::Abs(arg_name, Some(anno), body) => {
                CoreExpr::abs_anno(arg_name.clone(), self.zonk_ty(anno), self.zonk_expr(body))
            }
            CoreExpr::TyAbs(arg_name, body) => {
                CoreExpr::TyAbs(arg_name.clone(), Box::new(self.zonk_expr(body)))
            }
            CoreExpr::Let(bind_name, bind_body, in_expr) => CoreExpr::Let(
                bind_name.clone(),
                Box::new(self.zonk_expr(bind_body)),
                Box::new(self.zonk_expr(in_expr)),
            ),
            CoreExpr::Case(scrutinee, arms) => CoreExpr::Case(
                Box::new(self.zonk_expr(scrutinee)),
                arms.iter()
                    .map(|arm| CoreCaseArm {
                        pattern: arm.pattern.clone(),
                        expr: self.zonk_expr(&arm.expr),
                    })
                    .collect(),
            ),
            CoreExpr::IfThenElse(cond_br, then_br, else_br) => CoreExpr::IfThenElse(
                Box::new(self.zonk_expr(cond_br)),
                Box::new(self.zonk_expr(then_br)),
                Box::new(self.zonk_expr(else_br)),
            ),
        }
    }

    fn solve_subtype(&mut self, ctx: Context, ty_a: &Ty, ty_b: &Ty) -> Result<Context, String> {
        assert!(
            ty_well_formed(&ty_a, &ctx) && ty_well_formed(&ty_b, &ctx),
            "{}",
            format!("ill-formed subtype {} <: {}", ty_a.pretty(), ty_b.pretty())
        );

        let ty_a = &ctx.apply(ty_a);
        let ty_b = &ctx.apply(ty_b);

        match (ty_a, ty_b) {
            (Ty::TyExVar(evar_a), Ty::TyExVar(evar_b)) if evar_a == evar_b => Ok(ctx.clone()),
            (Ty::TyVar(ty_var_a), Ty::TyVar(ty_var_b)) if ty_var_a == ty_var_b => Ok(ctx.clone()),
            (Ty::TyCon(ctor_name_a), Ty::TyCon(ctor_name_b)) if ctor_name_a == ctor_name_b => {
                Ok(ctx.clone())
            }
            (Ty::TyUnit, Ty::TyUnit) => Ok(ctx.clone()),
            (Ty::TyArr(a1, a2), Ty::TyArr(b1, b2)) => {
                self.trace_rule("<:->");
                let theta_ctx = self.solve_subtype(ctx, b1, a1)?;
                let delta_ctx = self.solve_subtype(
                    theta_ctx.clone(),
                    &theta_ctx.apply(&a2),
                    &theta_ctx.apply(&b2),
                )?;

                Ok(delta_ctx)
            }
            (Ty::TyApp(a1, a2), Ty::TyApp(b1, b2)) => {
                self.trace_rule("<:$");
                let theta_ctx = self.solve_subtype(ctx, b1, a1)?;
                let delta_ctx = self.solve_subtype(
                    theta_ctx.clone(),
                    &theta_ctx.apply(&a2),
                    &theta_ctx.apply(&b2),
                )?;

                Ok(delta_ctx)
            }

            (Ty::TyForAll(quant_var, body), right_subtype) => {
                self.trace_rule("<: forAllL");
                let fresh_evar = self.make_fresh_evar();
                let evar_marker = ContextEntry::Marker {
                    var_name: fresh_evar.clone(),
                };
                let extended_ctx = ctx
                    .add_entry(evar_marker.clone())
                    .add_entry(ContextEntry::ExVar(fresh_evar.clone()))
                    .clone();

                let subst_body = body.substitute(quant_var, &Ty::TyExVar(fresh_evar.clone()));
                let theta = self.solve_subtype(extended_ctx, &subst_body, &right_subtype)?;

                Ok(theta.until(&evar_marker))
            }
            (left_subtype, Ty::TyForAll(quant_var, body)) => {
                self.trace_rule("<: forAllR");
                let ctx_mono_var = ContextEntry::MonoVar(quant_var.clone());
                let new_ctx = ctx.add_entry(ctx_mono_var.clone());

                let theta = self.solve_subtype(new_ctx, &left_subtype, &body)?;

                Ok(theta.until(&ctx_mono_var))
            }
            (Ty::TyExVar(evar), ty) => {
                self.trace_rule("<: instantiateL");
                if ty.occurs_check(evar.clone()) {
                    return Err(format!("occurs check error instantiateL"));
                };
                self.instantiate_left(ctx, evar.clone(), ty_b)
            }
            (ty, Ty::TyExVar(evar)) => {
                self.trace_rule("<: instantiateR");
                if ty.occurs_check(evar.clone()) {
                    return Err(format!(
                        "occurs check error instantiateR: {} <: {}",
                        ty_a.pretty(),
                        ty_b.pretty()
                    ));
                };
                self.instantiate_right(ctx, ty_a, evar.clone())
            }
            (lhs, rhs) => Err(format!(
                "unknown subtyping relation {} <: {}",
                lhs.pretty(),
                rhs.pretty()
            )),
        }
    }

    fn instantiate_left(
        &mut self,
        ctx: Context,
        left_evar: String,
        to_type: &Ty,
    ) -> Result<Context, String> {
        let (mut gamma_l, _, gamma_r) = ctx.split_2(&ContextEntry::ExVar(left_evar.clone()));
        if ty_well_formed(&to_type, &Context::from_entries(gamma_l.clone())) && to_type.is_mono() {
            self.trace_rule("instLSolve");

            let solution = ContextEntry::SolvedEVar {
                lhs: left_evar.clone(),
                rhs: to_type.clone(),
            };
            self.record_solution(&solution);
            let mut gamma = gamma_l.clone();
            gamma.push(solution);
            gamma.extend(gamma_r);
            return Ok(Context::from_entries(gamma));
        }

        match to_type {
            Ty::TyForAll(quantifier, body) => {
                self.trace_rule("InstLAllR");
                let quant_ctx_entry = ContextEntry::MonoVar(quantifier.clone());
                let extended_ctx = ctx.add_entry(quant_ctx_entry.clone());
                let delta = self
                    .instantiate_left(extended_ctx, left_evar, body)?
                    .until(&quant_ctx_entry);

                Ok(delta)
            }
            Ty::TyArr(b1, b2) => {
                self.trace_rule("instLArr");
                let a2_ex_var = self.make_fresh_evar();
                let a1_ex_var = self.make_fresh_evar();
                let solved_evar = ContextEntry::SolvedEVar {
                    lhs: left_evar.clone(),
                    rhs: Ty::make_arr(
                        Ty::TyExVar(a1_ex_var.clone()),
                        Ty::TyExVar(a2_ex_var.clone()),
                    ),
                };
                self.record_solution(&solved_evar);
                gamma_l.push(ContextEntry::ExVar(a2_ex_var.clone()));
                gamma_l.push(ContextEntry::ExVar(a1_ex_var.clone()));
                gamma_l.push(solved_evar);

                let new_ctx = Context::from_2(gamma_l, gamma_r);
                let theta = self.instantiate_right(new_ctx, b1, a1_ex_var.clone())?;
                let delta =
                    self.instantiate_left(theta.clone(), a2_ex_var.clone(), &theta.apply(b2))?;
                Ok(delta)
            }

            Ty::TyApp(t1, t2) => {
                self.trace_rule("instLApp");
                let a2_ex_var = self.make_fresh_evar();
                let a1_ex_var = self.make_fresh_evar();
                let solved_evar = ContextEntry::SolvedEVar {
                    lhs: left_evar.clone(),
                    rhs: Ty::make_app(
                        Ty::TyExVar(a1_ex_var.clone()),
                        Ty::TyExVar(a2_ex_var.clone()),
                    ),
                };
                self.record_solution(&solved_evar);
                gamma_l.push(ContextEntry::ExVar(a2_ex_var.clone()));
                gamma_l.push(ContextEntry::ExVar(a1_ex_var.clone()));
                gamma_l.push(solved_evar);

                let new_ctx = Context::from_2(gamma_l, gamma_r);
                let theta = self.instantiate_left(new_ctx, a1_ex_var.clone(), t1)?;
                let delta =
                    self.instantiate_left(theta.clone(), a2_ex_var.clone(), &theta.apply(t2))?;
                Ok(delta)
            }
            Ty::TyExVar(right_evar) => {
                self.trace_rule("instLReach");
                let left_first = &ctx.evar_before(&left_evar, right_evar);

                let gamma = if *left_first {
                    let solution = &ContextEntry::SolvedEVar {
                        lhs: right_evar.clone(),
                        rhs: Ty::TyExVar(left_evar.clone()),
                    };
                    self.record_solution(solution);
                    ctx.replace(&ContextEntry::ExVar(right_evar.clone()), solution)
                } else {
                    let solution = &ContextEntry::SolvedEVar {
                        lhs: left_evar.clone(),
                        rhs: Ty::TyExVar(right_evar.clone()),
                    };
                    self.record_solution(solution);
                    ctx.replace(&ContextEntry::ExVar(left_evar.clone()), solution)
                };

                Ok(gamma)
            }
            _ => todo!(),
        }
    }

    fn instantiate_right(
        &mut self,
        ctx: Context,
        to_type: &Ty,
        right_evar: String,
    ) -> Result<Context, String> {
        let (mut gamma_l, _, gamma_r) = ctx.split_2(&ContextEntry::ExVar(right_evar.clone()));
        if ty_well_formed(&to_type, &Context::from_entries(gamma_l.clone())) && to_type.is_mono() {
            self.trace_rule("instRSolve");

            let solution = ContextEntry::SolvedEVar {
                lhs: right_evar.clone(),
                rhs: to_type.clone(),
            };
            self.record_solution(&solution);
            let mut gamma = gamma_l.clone();
            gamma.push(solution);
            gamma.extend(gamma_r);

            return Ok(Context::from_entries(gamma));
        }
        match to_type {
            Ty::TyForAll(quantifier, body) => {
                self.trace_rule("instRAllL");
                let beta_var = self.make_fresh_evar();
                let beta_ctx_evar = ContextEntry::ExVar(beta_var.clone());
                let beta_ctx_marker = ContextEntry::Marker {
                    var_name: beta_var.clone(),
                };

                let gamma = ctx
                    .add_entry(beta_ctx_marker.clone())
                    .add_entry(beta_ctx_evar);

                let delta = self.instantiate_right(
                    gamma,
                    &body.substitute(quantifier, &Ty::TyExVar(beta_var)),
                    right_evar,
                )?;

                Ok(delta.until(&beta_ctx_marker))
            }
            Ty::TyArr(b1, b2) => {
                self.trace_rule("instRArr");
                let a2_ex_var = self.make_fresh_evar();
                let a1_ex_var = self.make_fresh_evar();
                let solved_evar = ContextEntry::SolvedEVar {
                    lhs: right_evar.clone(),
                    rhs: Ty::make_arr(
                        Ty::TyExVar(a1_ex_var.clone()),
                        Ty::TyExVar(a2_ex_var.clone()),
                    ),
                };
                self.record_solution(&solved_evar);
                gamma_l.push(ContextEntry::ExVar(a2_ex_var.clone()));
                gamma_l.push(ContextEntry::ExVar(a1_ex_var.clone()));
                gamma_l.push(solved_evar);

                let new_ctx = Context::from_2(gamma_l, gamma_r);
                let theta = self.instantiate_left(new_ctx, a1_ex_var.clone(), b1)?;
                let delta =
                    self.instantiate_right(theta.clone(), &theta.apply(b2), a2_ex_var.clone())?;
                Ok(delta)
            }
            Ty::TyApp(t1, t2) => {
                self.trace_rule("instRApp");
                let a2_ex_var = self.make_fresh_evar();
                let a1_ex_var = self.make_fresh_evar();
                let solved_evar = ContextEntry::SolvedEVar {
                    lhs: right_evar.clone(),
                    rhs: Ty::make_app(
                        Ty::TyExVar(a1_ex_var.clone()),
                        Ty::TyExVar(a2_ex_var.clone()),
                    ),
                };
                self.record_solution(&solved_evar);
                gamma_l.push(ContextEntry::ExVar(a2_ex_var.clone()));
                gamma_l.push(ContextEntry::ExVar(a1_ex_var.clone()));
                gamma_l.push(solved_evar);

                let new_ctx = Context::from_2(gamma_l, gamma_r);
                let theta = self.instantiate_left(new_ctx, a1_ex_var.clone(), t1)?;
                let delta =
                    self.instantiate_right(theta.clone(), &theta.apply(t2), a2_ex_var.clone())?;
                Ok(delta)
            }
            Ty::TyExVar(left_evar) => {
                self.trace_rule("instRReach");

                let left_first = &ctx.evar_before(left_evar, &right_evar);

                let gamma = if *left_first {
                    let left_solution = &ContextEntry::SolvedEVar {
                        lhs: right_evar.clone(),
                        rhs: Ty::TyExVar(left_evar.clone()),
                    };
                    self.record_solution(left_solution);
                    ctx.replace(&ContextEntry::ExVar(right_evar.clone()), left_solution)
                } else {
                    let right_solution = &ContextEntry::SolvedEVar {
                        lhs: left_evar.clone(),
                        rhs: Ty::TyExVar(right_evar.clone()),
                    };
                    self.record_solution(right_solution);
                    ctx.replace(&ContextEntry::ExVar(left_evar.clone()), right_solution)
                };

                Ok(gamma)
            }
            _ => todo!(
                "unhandled right instantiation case {} => {}",
                right_evar,
                to_type.pretty()
            ),
        }
    }

    fn force_arrow(&mut self, ctx: Context, ty: &Ty) -> Result<(Context, Ty, Ty), String> {
        let ty = ctx.apply(ty);

        match ty {
            Ty::TyArr(a, b) => Ok((ctx, (*a).clone(), (*b).clone())),
            Ty::TyForAll(q, body) => {
                let alpha = self.make_fresh_evar();
                let ctx = ctx.add_entry(ContextEntry::ExVar(alpha.clone()));
                self.force_arrow(ctx, &body.substitute(&q, &Ty::TyExVar(alpha)))
            }
            Ty::TyExVar(e) => {
                let (mut gl, _, gr) = ctx.split_2(&ContextEntry::ExVar(e.clone()));
                let a1 = self.make_fresh_evar();
                let a2 = self.make_fresh_evar();
                gl.push(ContextEntry::ExVar(a2.clone()));
                gl.push(ContextEntry::ExVar(a1.clone()));
                let solved_evar = ContextEntry::SolvedEVar {
                    lhs: e.clone(),
                    rhs: Ty::make_arr(Ty::TyExVar(a1.clone()), Ty::TyExVar(a2.clone())),
                };
                self.record_solution(&solved_evar);
                gl.push(solved_evar);
                let ctx2 = Context::from_2(gl, gr);
                Ok((ctx2, Ty::TyExVar(a1), Ty::TyExVar(a2)))
            }

            _ => Err(format!("Constructor is not applicable: {}", ty.pretty())),
        }
    }

    fn check_pattern(
        &mut self,
        ctx: Context,
        pattern: &CorePattern,
        expected_ty: &Ty,
    ) -> Result<(Context, Vec<(String, Ty)>), String> {
        let expected_ty = ctx.apply(expected_ty);

        match pattern {
            CorePattern::Wild => Ok((ctx, vec![])),
            CorePattern::Var(name) => Ok((ctx, vec![(name.clone(), expected_ty)])),
            CorePattern::Con(ctor_name, sub_patterns) => {
                let ctor_ty = self
                    .get_data_ctor_ty(ctor_name.clone())
                    .ok_or_else(|| format!("Unknown constructor in pattern: {}", ctor_name))?;

                let mut theta = ctx;
                let mut curr_ctor_ty = ctor_ty;
                let mut field_tys = Vec::with_capacity(sub_patterns.len());

                for _ in 0..sub_patterns.len() {
                    let forced = self.force_arrow(theta, &curr_ctor_ty)?;
                    let (f_theta, domain, codomain) = forced;

                    theta = f_theta;
                    field_tys.push(domain);
                    curr_ctor_ty = codomain;
                }

                theta = self.solve_subtype(
                    theta.clone(),
                    &theta.apply(&curr_ctor_ty),
                    &theta.apply(&expected_ty),
                )?;

                let mut bindings = Vec::new();
                for (sub_pat_ty, field_ty) in sub_patterns.iter().zip(field_tys.iter()) {
                    let field_ty = &theta.apply(field_ty);
                    let (ctx2, pat_tys) = self.check_pattern(theta, sub_pat_ty, &field_ty)?;
                    theta = ctx2;
                    bindings.extend(pat_tys);
                }

                Ok((theta, bindings))
            }
        }
    }

    fn solve_checking(
        &mut self,
        ctx: Context,
        expr: &CoreExpr,
        check_against: &Ty,
    ) -> Result<(Context, CoreExpr), String> {
        assert!(
            ty_well_formed(&check_against, &ctx),
            "can't check expression {:#?} against the malformed type {}",
            expr,
            check_against.pretty()
        );

        match (expr, check_against) {
            (CoreExpr::Unit, Ty::TyUnit) => {
                self.trace_rule("1I");
                Ok((ctx.clone(), CoreExpr::unit()))
            }
            (CoreExpr::TyAbs(alpha, body), Ty::TyForAll(quantifier, ty_body)) => {
                self.trace_rule("TyAbsI⇐");
                let subst_body_ty = ty_body.substitute(quantifier, &Ty::TyVar(alpha.clone()));
                let new_ctx = ctx.add_entry(ContextEntry::MonoVar(alpha.clone()));
                let (delta, elab_body) = self.solve_checking(new_ctx, body, &subst_body_ty)?;

                Ok((
                    delta.until(&ContextEntry::MonoVar(alpha.clone())),
                    CoreExpr::TyAbs(alpha.clone(), Box::new(elab_body)),
                ))
            }
            (CoreExpr::Abs(arg, anno, body), Ty::TyArr(a1, a2)) => {
                self.trace_rule("->I");

                let ctx = if let Some(anno_ty) = anno {
                    if !ty_well_formed(anno_ty, &ctx) {
                        return Err(format!(
                            "ill-formed type annotation {} on parameter {}",
                            anno_ty.pretty(),
                            arg,
                        ));
                    }

                    let ctx =
                        self.solve_subtype(ctx.clone(), &ctx.apply(a1), &ctx.apply(anno_ty))?;
                    self.solve_subtype(ctx.clone(), &ctx.apply(anno_ty), &ctx.apply(a1))?
                } else {
                    ctx
                };

                let typed_var_entry = ContextEntry::TypedVar {
                    var_name: arg.clone(),
                    ty: *a1.clone(),
                };
                let new_ctx = ctx.add_entry(typed_var_entry.clone());
                let (check_result, elab_body) = self.solve_checking(new_ctx, body, &a2.clone())?;
                Ok((
                    check_result.until(&typed_var_entry),
                    CoreExpr::abs_anno(arg.clone(), *a1.clone(), elab_body),
                ))
            }
            (CoreExpr::IfThenElse(cond, then_br, else_br), check_against) => {
                self.trace_rule("If⇐");

                let bool_ty = Ty::TyCon("Bool".to_string());
                let (gamma, elab_cond) = self.solve_checking(ctx, cond, &bool_ty)?;
                let (gamma, elab_then_br) = self.solve_checking(gamma, then_br, check_against)?;
                let (gamma, elab_else_br) = self.solve_checking(gamma, else_br, check_against)?;

                Ok((
                    gamma,
                    CoreExpr::IfThenElse(
                        Box::new(elab_cond),
                        Box::new(elab_then_br),
                        Box::new(elab_else_br),
                    ),
                ))
            }
            (_, Ty::TyForAll(quantifier, body)) => {
                self.trace_rule("forAllI");
                let new_ctx = ctx.add_entry(ContextEntry::MonoVar(quantifier.clone()));
                let (check_result, elab_expr) = self.solve_checking(new_ctx, expr, body)?;

                Ok((
                    check_result.until(&ContextEntry::MonoVar(quantifier.clone())),
                    CoreExpr::TyAbs(quantifier.clone(), Box::new(elab_expr)),
                ))
            }
            (_, _) => {
                self.trace_rule("Sub");
                let (infer_ctx, elab_expr, infer_ty) = self.solve_inference(ctx, expr)?;
                let subtype_result = self.solve_subtype(
                    infer_ctx.clone(),
                    &infer_ctx.apply(&infer_ty),
                    &infer_ctx.apply(&check_against),
                )?;

                Ok((subtype_result, elab_expr))
            }
        }
    }

    fn solve_app_inference(
        &mut self,
        ctx: Context,
        lhs_type: &Ty,
        expr: &CoreExpr,
        elab_result: &CoreExpr,
    ) -> Result<(Context, CoreExpr, Ty), String> {
        assert!(
            ty_well_formed(&lhs_type, &ctx),
            "{}",
            format!(
                "ill-formed application type {} in {:?}",
                lhs_type.pretty(),
                ctx
            )
        );
        match (lhs_type, expr) {
            (Ty::TyExVar(evar), _) => {
                self.trace_rule("existApp");
                let (mut gamma_l, _, gamma_r) = ctx.split_2(&ContextEntry::ExVar(evar.clone()));
                let alpha_2 = self.make_fresh_evar();
                let alpha_1 = self.make_fresh_evar();
                gamma_l.push(ContextEntry::ExVar(alpha_2.clone()));
                gamma_l.push(ContextEntry::ExVar(alpha_1.clone()));
                let solved_evar = ContextEntry::SolvedEVar {
                    lhs: evar.clone(),
                    rhs: Ty::make_arr(Ty::TyExVar(alpha_1.clone()), Ty::TyExVar(alpha_2.clone())),
                };
                self.record_solution(&solved_evar);
                gamma_l.push(solved_evar);

                let merged_ctx = Context::from_2(gamma_l, gamma_r);
                let (delta_result, elab_arg) =
                    self.solve_checking(merged_ctx, expr, &Ty::TyExVar(alpha_1.clone()))?;

                Ok((
                    delta_result,
                    CoreExpr::app(elab_result.clone(), elab_arg),
                    Ty::TyExVar(alpha_2.clone()),
                ))
            }
            (Ty::TyForAll(quantifier, body), _) => {
                self.trace_rule("forAllApp");
                let alpha = self.make_fresh_evar();
                let alpha_ctx_evar = ContextEntry::ExVar(alpha.clone());
                let instantiated_body = &body.substitute(quantifier, &Ty::TyExVar(alpha.clone()));
                let new_ctx = ctx.add_entry(alpha_ctx_evar);

                let elab_with_tyapp =
                    CoreExpr::TyApp(Box::new(elab_result.clone()), Ty::TyExVar(alpha.clone()));

                self.solve_app_inference(new_ctx, instantiated_body, expr, &elab_with_tyapp)
            }
            (Ty::TyArr(a1, a2), _) => {
                self.trace_rule("arrApp");
                let (checked_result, elab_arg) = self.solve_checking(ctx, expr, a1)?;
                Ok((
                    checked_result,
                    CoreExpr::app(elab_result.clone(), elab_arg),
                    *a2.clone(),
                ))
            }

            _ => todo!(
                "unacceptable app inference case, {:?} : {}",
                expr,
                lhs_type.clone().pretty()
            ),
        }
    }

    pub fn solve_inference(
        &mut self,
        ctx: Context,
        expr: &CoreExpr,
    ) -> Result<(Context, CoreExpr, Ty), String> {
        match expr {
            CoreExpr::Unit => Ok((ctx.clone(), CoreExpr::Unit, Ty::TyUnit)),
            CoreExpr::Var(name) => {
                self.trace_rule("Var");
                if let Some(ContextEntry::TypedVar { var_name: _, ty }) =
                    ctx.get_typed_var(name.clone())
                {
                    return Ok((ctx.clone(), CoreExpr::var(name.clone()), ty.clone()));
                }

                if let Some(ty) = self.get_func_type(name.clone()) {
                    return Ok((ctx.clone(), CoreExpr::var(name.clone()), ty.clone()));
                }

                Err(format!("variable {} not in scope", name))
            }
            CoreExpr::Abs(arg, anno, body) => {
                self.trace_rule("I=>");

                let (arg_ty, mut new_ctx) = match anno {
                    Some(anno_ty) => {
                        if !ty_well_formed(anno_ty, &ctx) {
                            return Err(format!(
                                "ill-formed type annotation {} on parameter {}",
                                anno_ty.pretty(),
                                arg,
                            ));
                        }
                        (anno_ty.clone(), ctx)
                    }
                    None => {
                        let alpha = self.make_fresh_evar();
                        let new_ctx = ctx.add_entry(ContextEntry::ExVar(alpha.clone()));
                        (Ty::TyExVar(alpha), new_ctx)
                    }
                };

                let beta = self.make_fresh_evar();
                let arg_entry = ContextEntry::TypedVar {
                    var_name: arg.clone(),
                    ty: arg_ty.clone(),
                };

                new_ctx = new_ctx
                    .add_entry(ContextEntry::ExVar(beta.clone()))
                    .add_entry(arg_entry.clone());

                let (delta, elab_body) =
                    self.solve_checking(new_ctx, body, &Ty::TyExVar(beta.clone()))?;

                Ok((
                    delta.until(&arg_entry),
                    CoreExpr::abs_anno(arg.clone(), arg_ty.clone(), elab_body),
                    Ty::make_arr(arg_ty, Ty::TyExVar(beta)),
                ))
            }
            CoreExpr::App(left, right) => {
                self.trace_rule("->E");
                let (inf_left_ctx, inf_left_expr, inf_left_ty) = self.solve_inference(ctx, left)?;
                let ctx_applied_to_inf = &inf_left_ctx.apply(&inf_left_ty);
                let inf_app = self.solve_app_inference(
                    inf_left_ctx,
                    &ctx_applied_to_inf,
                    right,
                    &inf_left_expr,
                )?;

                Ok(inf_app)
            }
            CoreExpr::Anno(ann_expr, assigned_ty) => {
                assert!(
                    ty_well_formed(assigned_ty, &ctx),
                    "{}",
                    format!(
                        "ill-formed type in annotation {:?}:{}",
                        ann_expr.clone(),
                        assigned_ty.pretty()
                    )
                );
                let (delta, elab_expr) = self.solve_checking(ctx, ann_expr, assigned_ty)?;
                Ok((
                    delta,
                    CoreExpr::anno(elab_expr.clone(), assigned_ty.clone()),
                    assigned_ty.clone(),
                ))
            }
            CoreExpr::Let(bind_name, bind_expr, in_expr) => {
                let (gamma, elab_bind_expr, bind_expr_ty) = self.solve_inference(ctx, bind_expr)?;
                let typed_var_ctx_entry = ContextEntry::TypedVar {
                    var_name: bind_name.into(),
                    ty: bind_expr_ty,
                };
                let theta = gamma.add_entry(typed_var_ctx_entry.clone());
                let (delta, elab_in_expr, in_expr_ty) = self.solve_inference(theta, in_expr)?;

                Ok((
                    delta.remove_in_place(&typed_var_ctx_entry.clone()),
                    CoreExpr::let_expr(bind_name.clone(), elab_bind_expr, elab_in_expr),
                    in_expr_ty,
                ))
            }
            CoreExpr::Con(name) => {
                let ctor_ty = self.get_data_ctor_ty(name.clone()).unwrap();

                Ok((ctx.clone(), CoreExpr::Con(name.clone()), ctor_ty))
            }
            CoreExpr::Case(scrutinee, arms) => {
                self.trace_rule("Case⇒");

                if arms.is_empty() {
                    return Err("Case with no arms".to_string());
                }

                let (gamma, elab_scrutinee, scrut_ty0) = self.solve_inference(ctx, scrutinee)?;
                let mut theta = gamma;
                let scrut_ty0 = theta.apply(&scrut_ty0);

                let result_evar = self.make_fresh_evar();
                let result_ty = Ty::TyExVar(result_evar.clone());
                theta = theta.add_entry(ContextEntry::ExVar(result_evar.clone()));

                let mut elab_arms = Vec::with_capacity(arms.len());

                for (arm_ix, arm) in arms.iter().enumerate() {
                    let marker = ContextEntry::Marker {
                        var_name: format!("case_arm#{}", arm_ix),
                    };

                    let arm_base_ctx = theta.clone().add_entry(marker.clone());
                    let scrut_ty = arm_base_ctx.apply(&scrut_ty0);

                    let (delta, bindings) =
                        self.check_pattern(arm_base_ctx, &arm.pattern, &scrut_ty)?;

                    let mut arm_ctx = delta;
                    for (var_name, var_ty) in bindings {
                        arm_ctx = arm_ctx.add_entry(ContextEntry::TypedVar {
                            var_name,
                            ty: var_ty,
                        });
                    }

                    if !arm_ctx.has_evar(result_evar.clone())
                        && !arm_ctx.has_solved_evar(result_evar.clone())
                    {
                        return Err(format!(
                            "case result existential {} missing from arm context (arm {})",
                            result_evar, arm_ix
                        ));
                    }

                    let res_ty = arm_ctx.apply(&result_ty);
                    let (omega, elab_arm_body) =
                        self.solve_checking(arm_ctx, &arm.expr, &res_ty)?;
                    theta = omega.until(&marker);

                    elab_arms.push(CoreCaseArm {
                        pattern: arm.pattern.clone(),
                        expr: elab_arm_body,
                    });
                }

                let final_ty = theta.apply(&result_ty);
                let elab = CoreExpr::Case(Box::new(elab_scrutinee), elab_arms);
                Ok((theta, elab, final_ty))
            }
            CoreExpr::IfThenElse(cond, then_br, else_br) => {
                self.trace_rule("If⇒");

                let bool_ty = Ty::TyCon("Bool".to_string());
                let (gamma, cond_elab, cond_ty) = self.solve_inference(ctx, cond)?;
                let gamma = self.solve_subtype(gamma.clone(), &gamma.apply(&cond_ty), &bool_ty)?;

                let res_evar = self.make_fresh_evar();
                let res_ty = Ty::TyExVar(res_evar.clone());
                let theta = gamma.add_entry(ContextEntry::ExVar(res_evar.clone()));

                let (theta, then_br_elab) = self.solve_checking(theta, then_br, &res_ty)?;
                let (theta, else_br_elab) = self.solve_checking(theta, else_br, &res_ty)?;

                let final_ty = theta.apply(&res_ty);
                Ok((
                    theta,
                    CoreExpr::IfThenElse(
                        Box::new(cond_elab),
                        Box::new(then_br_elab),
                        Box::new(else_br_elab),
                    ),
                    final_ty,
                ))
            }
            CoreExpr::TyApp(inner, ty_arg) => {
                self.trace_rule("TyApp⇒");
                assert!(
                    ty_well_formed(ty_arg, &ctx),
                    "ill-formed type argument {}",
                    ty_arg.pretty()
                );

                let (delta, elab_inner, inner_ty) = self.solve_inference(ctx, inner)?;
                let applied_ty = delta.apply(&inner_ty);

                match applied_ty {
                    Ty::TyForAll(quantifier, body) => {
                        let result_ty = body.substitute(&quantifier, ty_arg);
                        Ok((
                            delta,
                            CoreExpr::TyApp(Box::new(elab_inner), ty_arg.clone()),
                            result_ty,
                        ))
                    }
                    _ => Err(format!(
                        "type application to non-polymorphic type: {} [{}]",
                        applied_ty.pretty(),
                        ty_arg.pretty()
                    )),
                }
            }
            CoreExpr::TyAbs(alpha, body) => {
                self.trace_rule("TyAbsI⇒");
                let mono_entry = ContextEntry::MonoVar(alpha.clone());
                let new_ctx = ctx.add_entry(mono_entry.clone());
                let (delta, elab_body, body_ty) = self.solve_inference(new_ctx, body)?;

                Ok((
                    delta.until(&mono_entry),
                    CoreExpr::TyAbs(alpha.clone(), Box::new(elab_body)),
                    Ty::make_forall(alpha.clone(), body_ty),
                ))
            }
        }
    }
}

pub fn check_module(module: CoreModule) {
    let mut inference =
        TypeInference::with_module_context(module.data_ctors.clone(), module.func_types.clone());
    let mut ctx = Context::new();

    println!("── Checking module ──\n");

    for func_def in module.func_defns.iter() {
        let sig = func_def.signature.pretty();

        let (new_ctx, elaborated) = inference
            .solve_checking(ctx, &func_def.body, &func_def.signature)
            .unwrap_or_else(|e| panic!("  ✗ {} : {}\n    type error: {}", func_def.name, sig, e));

        ctx = new_ctx;

        let zonked = inference.zonk_expr(&elaborated);

        let mut checker = TypeInference::with_module_context(
            module.data_ctors.clone(),
            module.func_types.clone(),
        );

        checker
            .solve_checking(Context::new(), &zonked, &func_def.signature)
            .unwrap_or_else(|e| {
                panic!(
                    "  ✗ {} : {}\n    elaboration invalid: {}",
                    func_def.name, sig, e
                )
            });

        println!("  ✓ {} : {}", func_def.name, sig);
    }

    println!("\n── All definitions verified ──");
}
