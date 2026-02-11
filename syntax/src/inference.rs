use crate::context::{Context, ContextEntry};
use core::{
    core_expr::{CoreDefinition, CoreExpr},
    core_type::Ty,
};
use std::iter::successors;

pub fn apply_context_once(ctx: &Context, ty: &Ty) -> Ty {
    match ty {
        Ty::TyVar(_) | Ty::TyCon(_) => ty.clone(),
        Ty::TyUnit => ty.clone(),
        Ty::TyArr(lhs, rhs) => {
            let app_lhs = apply_context(ctx, lhs);
            let app_rhs = apply_context(ctx, rhs);
            Ty::make_arr(app_lhs, app_rhs)
        }
        Ty::TyApp(lhs, rhs) => {
            let app_lhs = apply_context(ctx, lhs);
            let app_rhs = apply_context(ctx, rhs);
            Ty::make_app(app_lhs, app_rhs)
        }
        Ty::TyExVar(ex_var_name) => {
            if let Some(ContextEntry::SolvedEVar {
                lhs: _,
                rhs: solved_type,
            }) = ctx.find_solved_evar(ex_var_name.clone())
            {
                return apply_context(ctx, &solved_type);
            };

            ty.clone()
        }
        Ty::TyForAll(quant_var, body) => {
            Ty::make_forall(quant_var.clone(), apply_context(ctx, body))
        }
    }
}

pub fn apply_context(ctx: &Context, ty: &Ty) -> Ty {
    successors(Some(ty.clone()), |prev| {
        let next = apply_context_once(ctx, prev);
        if next != *prev { Some(next) } else { None }
    })
    .last()
    .unwrap()
}

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
        _ => todo!("add constructors!"),
    }
}

pub struct TypeInference {
    var_counter: usize,
}

impl TypeInference {
    pub fn new() -> TypeInference {
        Self { var_counter: 0 }
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
                    &apply_context(&theta_ctx, &a2),
                    &apply_context(&theta_ctx, &b2),
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
            (lhs, rhs) => Err(format!("unknown subtyping relation {:?} <: {:?}", lhs, rhs)),
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
                let delta = self.instantiate_left(
                    theta.clone(),
                    a2_ex_var.clone(),
                    &apply_context(&theta, b2),
                )?;
                Ok(delta)
            }
            Ty::TyExVar(right_evar) => {
                self.trace_rule("instLReach");
                let gamma = ctx.replace(
                    &ContextEntry::ExVar(right_evar.clone()),
                    &ContextEntry::SolvedEVar {
                        lhs: right_evar.clone(),
                        rhs: Ty::TyExVar(left_evar.clone()),
                    },
                );

                Ok(gamma.clone())
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
                let delta = self.instantiate_right(
                    theta.clone(),
                    &apply_context(&theta, b2),
                    a2_ex_var.clone(),
                )?;
                Ok(delta)
            }
            Ty::TyExVar(left_evar) => {
                self.trace_rule("instRReach");
                let gamma = ctx.replace(
                    &ContextEntry::ExVar(right_evar.clone()),
                    &ContextEntry::SolvedEVar {
                        lhs: right_evar.clone(),
                        rhs: Ty::TyExVar(left_evar.clone()),
                    },
                );

                Ok(gamma.clone())
            }
            _ => todo!("unhandled right instantiation case"),
        }
    }

    fn solve_checking(
        &mut self,
        ctx: Context,
        expr: &CoreExpr,
        check_against: &Ty,
    ) -> Result<Context, String> {
        assert!(ty_well_formed(&check_against, &ctx));
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
                    &apply_context(&infer_ctx, &infer_ty),
                    &apply_context(&infer_ctx, check_against),
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

            _ => todo!("unacceptable app inference case"),
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
                let ctx_applied_to_inf = apply_context(&inf_left_ctx, &inf_left_ty);
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
            _ => todo!("expressions like {:?} are not implemented", expr),
        }
    }

    pub fn check_program(&mut self, definitions: Vec<CoreDefinition>) -> Result<Context, String> {
        let mut ctx = Context::new();
        for defn in definitions.iter() {
            match defn {
                CoreDefinition::FunDef {
                    name,
                    signature,
                    body,
                } => {
                    if !ty_well_formed(signature, &ctx) {
                        return Err(format!(
                            "ill-formed top-level type for {}: {}",
                            name,
                            signature.pretty()
                        ));
                    }
                    let delta = self.solve_checking(ctx.clone(), body, signature)?;
                    ctx = delta.add_entry(ContextEntry::TypedVar {
                        var_name: name.clone(),
                        ty: signature.clone(),
                    })
                }
            }
        }
        Ok(ctx)
    }
}
