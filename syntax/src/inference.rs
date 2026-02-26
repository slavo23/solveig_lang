use crate::context::{Context, ContextEntry};
use core::{
    core_expr::{CoreExpr, CoreModule, CorePattern},
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
}

impl TypeInference {
    pub fn new() -> TypeInference {
        Self {
            var_counter: 0,
            data_ctors: Default::default(),
            func_types: Default::default(),
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

    fn trace_rule(&self, rule_name: &'static str) {
        println!("executing rule [{}]", rule_name)
    }

    fn solve_subtype(&mut self, ctx: Context, ty_a: &Ty, ty_b: &Ty) -> Result<Context, String> {
        assert!(
            ty_well_formed(&ty_a, &ctx) && ty_well_formed(&ty_b, &ctx),
            "{}",
            format!("ill-formed subtype {} <: {}", ty_a.pretty(), ty_b.pretty())
        );

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
                    ctx.replace(
                        &ContextEntry::ExVar(right_evar.clone()),
                        &ContextEntry::SolvedEVar {
                            lhs: right_evar.clone(),
                            rhs: Ty::TyExVar(left_evar.clone()),
                        },
                    )
                } else {
                    ctx.replace(
                        &ContextEntry::ExVar(left_evar.clone()),
                        &ContextEntry::SolvedEVar {
                            lhs: left_evar.clone(),
                            rhs: Ty::TyExVar(right_evar.clone()),
                        },
                    )
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
                    ctx.replace(
                        &ContextEntry::ExVar(right_evar.clone()),
                        &ContextEntry::SolvedEVar {
                            lhs: right_evar.clone(),
                            rhs: Ty::TyExVar(left_evar.clone()),
                        },
                    )
                } else {
                    ctx.replace(
                        &ContextEntry::ExVar(left_evar.clone()),
                        &ContextEntry::SolvedEVar {
                            lhs: left_evar.clone(),
                            rhs: Ty::TyExVar(right_evar.clone()),
                        },
                    )
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
                gl.push(ContextEntry::SolvedEVar {
                    lhs: e.clone(),
                    rhs: Ty::make_arr(Ty::TyExVar(a1.clone()), Ty::TyExVar(a2.clone())),
                });
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
    ) -> Result<Context, String> {
        assert!(
            ty_well_formed(&check_against, &ctx),
            "can't check expression {:#?} against the malformed type {}",
            expr,
            check_against.pretty()
        );

        match (expr, check_against) {
            (CoreExpr::Unit, Ty::TyUnit) => {
                self.trace_rule("1I");
                Ok(ctx.clone())
            }
            (CoreExpr::Abs(arg, body), Ty::TyArr(a1, a2)) => {
                self.trace_rule("->I");
                let typed_var_entry = ContextEntry::TypedVar {
                    var_name: arg.clone(),
                    ty: *a1.clone(),
                };
                let new_ctx = ctx.add_entry(typed_var_entry.clone());
                let check_result = self.solve_checking(new_ctx, body, &a2.clone())?;
                Ok(check_result.until(&typed_var_entry))
            }
            (_, Ty::TyForAll(quantifier, body)) => {
                self.trace_rule("forAllI");
                let new_ctx = ctx.add_entry(ContextEntry::MonoVar(quantifier.clone()));

                let check_result = self.solve_checking(new_ctx, expr, body)?;

                Ok(check_result.until(&ContextEntry::MonoVar(quantifier.clone())))
            }
            (_, _) => {
                self.trace_rule("Sub");
                let (infer_ctx, infer_ty) = self.solve_inference(ctx, expr)?;
                let subtype_result = self.solve_subtype(
                    infer_ctx.clone(),
                    &infer_ctx.apply(&infer_ty),
                    &infer_ctx.apply(&check_against),
                )?;

                Ok(subtype_result)
            }
        }
    }

    fn solve_app_inference(
        &mut self,
        ctx: Context,
        lhs_type: &Ty,
        expr: &CoreExpr,
    ) -> Result<(Context, Ty), String> {
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
                gamma_l.push(ContextEntry::SolvedEVar {
                    lhs: evar.clone(),
                    rhs: Ty::make_arr(Ty::TyExVar(alpha_1.clone()), Ty::TyExVar(alpha_2.clone())),
                });

                let merged_ctx = Context::from_2(gamma_l, gamma_r);
                let delta_result =
                    self.solve_checking(merged_ctx, expr, &Ty::TyExVar(alpha_1.clone()))?;

                Ok((delta_result, Ty::TyExVar(alpha_2.clone())))
            }
            (Ty::TyForAll(quantifier, body), _) => {
                self.trace_rule("forAllApp");
                let alpha = self.make_fresh_evar();
                let alpha_ctx_evar = ContextEntry::ExVar(alpha.clone());
                let new_ctx = ctx.add_entry(alpha_ctx_evar);

                self.solve_app_inference(
                    new_ctx,
                    &body.substitute(quantifier, &Ty::TyExVar(alpha.clone())),
                    expr,
                )
            }
            (Ty::TyArr(a1, a2), _) => {
                self.trace_rule("arrApp");
                let checked_result = self.solve_checking(ctx, expr, a1)?;
                Ok((checked_result, *a2.clone()))
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
    ) -> Result<(Context, Ty), String> {
        match expr {
            CoreExpr::Unit => Ok((ctx.clone(), Ty::TyUnit)),
            CoreExpr::Var(name) => {
                self.trace_rule("Var");
                if let Some(ContextEntry::TypedVar { var_name: _, ty }) =
                    ctx.get_typed_var(name.clone())
                {
                    return Ok((ctx.clone(), ty.clone()));
                }

                if let Some(ty) = self.get_func_type(name.clone()) {
                    return Ok((ctx.clone(), ty.clone()));
                }

                Err(format!("variable {} not in scope", name))
            }
            CoreExpr::Abs(arg, body) => {
                self.trace_rule("I=>");
                let alpha_ctx_evar = self.make_fresh_evar();
                let beta_ctx_evar = self.make_fresh_evar();
                let arg_ctx_evar = ContextEntry::TypedVar {
                    var_name: arg.clone(),
                    ty: Ty::TyExVar(alpha_ctx_evar.clone()),
                };

                let new_ctx = ctx
                    .add_entry(ContextEntry::ExVar(alpha_ctx_evar.clone()))
                    .add_entry(ContextEntry::ExVar(beta_ctx_evar.clone()))
                    .add_entry(arg_ctx_evar.clone());

                let checked =
                    self.solve_checking(new_ctx, body, &Ty::TyExVar(beta_ctx_evar.clone()))?;

                Ok((
                    checked.until(&arg_ctx_evar.clone()),
                    Ty::make_arr(
                        Ty::TyExVar(alpha_ctx_evar.clone()),
                        Ty::TyExVar(beta_ctx_evar.clone()),
                    ),
                ))
            }
            CoreExpr::App(left, right) => {
                self.trace_rule("->E");
                let (inf_left_ctx, inf_left_ty) = self.solve_inference(ctx, left)?;
                let ctx_applied_to_inf = &inf_left_ctx.apply(&inf_left_ty);
                let inf_app = self.solve_app_inference(inf_left_ctx, &ctx_applied_to_inf, right)?;

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
                let delta = self.solve_checking(ctx, ann_expr, assigned_ty)?;
                Ok((delta, assigned_ty.clone()))
            }
            CoreExpr::Let(bind_name, bind_expr, in_expr) => {
                let (gamma, bind_expr_ty) = self.solve_inference(ctx, bind_expr)?;
                let typed_var_ctx_entry = ContextEntry::TypedVar {
                    var_name: bind_name.into(),
                    ty: bind_expr_ty,
                };
                let theta = gamma.add_entry(typed_var_ctx_entry.clone());
                let (delta, in_expr_ty) = self.solve_inference(theta, in_expr)?;

                Ok((
                    delta.remove_in_place(&typed_var_ctx_entry.clone()),
                    in_expr_ty,
                ))
            }
            CoreExpr::Fix(inner) => {
                let alpha_evar_name = self.make_fresh_evar();
                let alpha_evar_ctx_entry = ContextEntry::ExVar(alpha_evar_name.clone());
                let alpha_evar_ty = Ty::TyExVar(alpha_evar_name.clone());
                let alpha_evar_fn = Ty::make_arr(alpha_evar_ty.clone(), alpha_evar_ty.clone());

                let gamma = ctx.add_entry(alpha_evar_ctx_entry);
                let delta = self.solve_checking(gamma, inner, &alpha_evar_fn)?;

                Ok((delta, alpha_evar_ty))
            }
            CoreExpr::Con(name, args) => {
                let ctor_ty = self.get_data_ctor_ty(name.clone()).unwrap();

                if args.is_empty() {
                    return Ok((ctx.clone(), ctor_ty.clone()));
                }

                let mut delta = ctx.clone();
                let mut current_ty = ctor_ty.clone();

                for arg in args.iter() {
                    let applied_ty = &delta.apply(&current_ty);
                    let (theta, result_ty) =
                        self.solve_app_inference(delta.clone(), &applied_ty, arg)?;
                    delta = theta;
                    current_ty = result_ty;
                }

                Ok((delta.clone(), current_ty.clone()))
            }

            CoreExpr::Case(scrutinee, arms) => {
                self.trace_rule("Case⇒");

                if arms.is_empty() {
                    return Err("Case with no arms".to_string());
                }

                let (gamma, scrut_ty0) = self.solve_inference(ctx, scrutinee)?;
                let mut theta = gamma;
                let scrut_ty0 = theta.apply(&scrut_ty0);

                let result_evar = self.make_fresh_evar();
                let result_ty = Ty::TyExVar(result_evar.clone());
                theta = theta.add_entry(ContextEntry::ExVar(result_evar.clone()));

                for (arm_ix, arm) in arms.iter().enumerate() {
                    let marker = ContextEntry::Marker {
                        var_name: format!("case_arm#{}", arm_ix),
                    };

                    let arm_base_ctx = theta.clone().add_entry(marker.clone());
                    let scrut_ty = &arm_base_ctx.apply(&scrut_ty0);

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

                    let res_ty = &arm_ctx.apply(&result_ty);
                    let omega = self.solve_checking(arm_ctx, &arm.expr, &res_ty)?;
                    theta = omega.until(&marker);
                }

                Ok((theta.clone(), theta.apply(&result_ty)))
            }
        }
    }
}

pub fn check_module(module: CoreModule) {
    let mut inference = TypeInference::with_module_context(module.data_ctors, module.func_types);
    let mut ctx = Context::new();
    for func_def in module.func_defns.iter() {
        ctx = inference
            .solve_checking(ctx, &func_def.body, &func_def.signature)
            .unwrap();
    }

    println!("{:#?}", ctx)
}
