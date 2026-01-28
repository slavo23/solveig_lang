use std::{env::temp_dir, ops::Index};

#[derive(Clone, PartialEq, Debug)]
enum Ty {
    TyUnit,
    TyVar(String),
    TyExVar(String),
    TyArr(Box<Ty>, Box<Ty>),
    TyForAll(String, Box<Ty>),
}

impl Ty {
    pub fn is_mono(&self) -> bool {
        match self {
            Ty::TyUnit => true,
            Ty::TyVar(_) | Ty::TyExVar(_) => true,
            Ty::TyArr(lhs, rhs) => lhs.is_mono() && rhs.is_mono(),
            Ty::TyForAll(_, _) => false,
        }
    }

    pub fn occurs_check(&self, var_name: String) -> bool {
        match self {
            Self::TyExVar(name) | Self::TyVar(name) if *name == var_name => true,
            Self::TyArr(lhs, rhs) => {
                lhs.occurs_check(var_name.clone()) || rhs.occurs_check(var_name)
            }
            Self::TyForAll(_, body) => body.occurs_check(var_name),
            _ => false,
        }
    }

    pub fn make_arr(lhs: Ty, rhs: Ty) -> Ty {
        Ty::TyArr(Box::new(lhs), Box::new(rhs))
    }

    pub fn make_var(var_name: String) -> Ty {
        Self::TyVar(var_name)
    }

    pub fn make_forall(quant_name: String, body: Ty) -> Ty {
        Self::TyForAll(quant_name, Box::new(body))
    }

    pub fn substitute(&self, var_name: &str, with_type: &Ty) -> Ty {
        match self {
            Self::TyVar(sub_var_name) if var_name == sub_var_name => with_type.clone(),
            Self::TyUnit => self.clone(),
            Self::TyArr(lhs, rhs) => Self::make_arr(
                lhs.substitute(var_name, with_type),
                rhs.substitute(var_name, with_type),
            ),
            Self::TyForAll(quantifier, body) => {
                if quantifier == var_name {
                    return self.clone();
                }
                Self::make_forall(quantifier.clone(), body.substitute(var_name, with_type))
            }
            _ => panic!("subst failed"),
        }
    }
}

#[derive(Clone, Debug)]
enum CoreExpr {
    Unit,
    Var(String),
    App(Box<CoreExpr>, Box<CoreExpr>),
    Abs(String, Box<CoreExpr>),
}

#[derive(Clone, PartialEq, Debug)]
enum ContextEntry {
    ExVar(String),
    MonoVar(String),
    TypedVar { var_name: String, ty: Ty },
    SolvedEVar { lhs: String, rhs: Ty },
    Marker { var_name: String },
}

#[derive(Clone, Debug)]
struct Context {
    entries: Vec<ContextEntry>,
}

impl Context {
    pub fn new() -> Context {
        Context { entries: vec![] }
    }

    pub fn add_entry(&mut self, entry: ContextEntry) -> &mut Context {
        self.entries.push(entry);
        self
    }

    pub fn find_solved_evar(&self, evar_name: String) -> Option<ContextEntry> {
        self.entries
            .iter()
            .find(|ent| {
                matches!(
                    ent, ContextEntry::SolvedEVar { lhs, rhs } if *lhs == evar_name
                )
            })
            .cloned()
    }

    pub fn until(&self, entry: &ContextEntry) -> Context {
        let (left, _, _) = self.split_2(entry);
        return Context { entries: left }
    }

    pub fn before(&self, entry_a: &ContextEntry, entry_b: &ContextEntry) -> bool {
        let found_a = self.entries.iter().position(|ent| ent == entry_a);
        let found_b = self.entries.iter().position(|ent| ent == entry_b);

        match (found_a, found_b) {
            (Some(a_ix), Some(b_ix)) => a_ix < b_ix,
            _ => false,
        }
    }

    pub fn replace(&mut self, entry: &ContextEntry, replacement: &ContextEntry) -> &mut Self {
        for i in 0..self.entries.len() {
            if self.entries[i] == *entry {
                self.entries[i] = replacement.clone();
            }
        }

        self
    }

    pub fn split_2(
        &self,
        on_entry: &ContextEntry,
    ) -> (Vec<ContextEntry>, &ContextEntry, Vec<ContextEntry>) {
        let found_ix = self.entries.iter().position(|ent| ent == on_entry);
        match found_ix {
            Some(ix) => {
                let middle = &self.entries[ix];
                let entries_copy = self.entries.clone();

                let left = entries_copy.get(0..ix).unwrap_or_default().to_vec();
                let right = &entries_copy.get(ix + 1..).unwrap_or_default().to_vec();

                (left.clone(), middle, right.clone())
            }
            None => {
                let err = format!("{:?}", on_entry);
                panic!("{} not found in the context", err);
            }
        }
    }

    pub fn from_2(entries_1: Vec<ContextEntry>, entries_2: Vec<ContextEntry>) -> Context {
        let mut left_ctx = entries_1.clone();
        left_ctx.extend(entries_2);
        Self { entries: left_ctx }
    }

    pub fn has_evar(&self, evar_name: String) -> bool {
        self.entries
            .iter()
            .find(|ent| {
                matches!(
                    ent, ContextEntry::ExVar(found_evar_name) if *found_evar_name == evar_name
                )
            })
            .is_some()
    }

    pub fn get_typed_var(&self, var_name: String) -> Option<&ContextEntry> {
        self.entries.iter().find(
            |ent| matches!(
                ent,
                ContextEntry::TypedVar { var_name: typed_var_name, ty: var_type } if var_name == *typed_var_name )
            )
    }
}

fn apply_context(ctx: &Context, ty: &Ty) -> Ty {
    match ty {
        Ty::TyVar(_) => ty.clone(),
        Ty::TyUnit => ty.clone(),
        Ty::TyArr(lhs, rhs) => {
            let app_lhs = apply_context(ctx, lhs);
            let app_rhs = apply_context(ctx, rhs);
            Ty::make_arr(app_lhs, app_rhs)
        }
        Ty::TyExVar(ex_var_name) => {
            if let Some(ContextEntry::SolvedEVar {
                lhs: _,
                rhs: solved_type,
            }) = ctx.find_solved_evar(ex_var_name.clone())
            {
                return apply_context(ctx, &solved_type);
            };

            return ty.clone()
        }
        Ty::TyForAll(quant_var, body) => {
            Ty::make_forall(quant_var.clone(), apply_context(ctx, body))
        }
    }
}

struct TypeInference {
    var_counter: usize,
}

struct InferenceResult {
    expr: CoreExpr,
    expr_ty: Ty,
    ctx: Context,
}

fn trace_rule(rule_name: &'static str) {
    println!("executing rule [{}]", rule_name)
}

impl TypeInference {
    pub fn new() -> TypeInference {
        Self { var_counter: 0 }
    }

    fn make_fresh_evar(&mut self) -> String {
        self.var_counter += 1;
        format!("a^{}", self.var_counter)
    }

    fn make_fresh_mono_var(&mut self) -> String {
        self.var_counter += 1;
        format!("a^{}", self.var_counter)
    }

    fn solve_subtype(
        &mut self,
        ctx: &mut Context,
        ty_a: &Ty,
        ty_b: &Ty,
    ) -> Result<Context, String> {
        match (ty_a, ty_b) {
            (Ty::TyExVar(evar_a), Ty::TyExVar(evar_b)) if evar_a == evar_b => Ok(ctx.clone()),
            (Ty::TyVar(ty_var_a), Ty::TyVar(ty_var_b)) if ty_var_a == ty_var_b => Ok(ctx.clone()),
            (Ty::TyUnit, Ty::TyUnit) => Ok(ctx.clone()),
            (Ty::TyArr(a1, a2), Ty::TyArr(b1, b2)) => {
                trace_rule("<:->");
                let theta_ctx = self.solve_subtype(ctx, b1, a1)?;
                let delta_ctx = self.solve_subtype(
                    &mut theta_ctx.clone(),
                    &apply_context(&theta_ctx, &a2),
                    &apply_context(&theta_ctx, &b2),
                )?;

                Ok(delta_ctx)
            }
            (Ty::TyForAll(quant_var, body), right_subtype) => {
                trace_rule("<: forAllL");
                let fresh_evar = self.make_fresh_evar();
                let evar_marker = ContextEntry::Marker {
                    var_name: fresh_evar.clone(),
                };
                let mut extended_ctx = ctx
                    .add_entry(evar_marker.clone())
                    .add_entry(ContextEntry::ExVar(fresh_evar.clone()))
                    .clone();

                let subst_body = body.substitute(quant_var, &Ty::TyExVar(fresh_evar.clone()));
                let theta = self.solve_subtype(&mut extended_ctx, &subst_body, &right_subtype)?;

                Ok(theta.until(&evar_marker))
            }
            (left_subtype, Ty::TyForAll(quant_var, body)) => {
                trace_rule("<: forAllR");
                let ctx_mono_var = ContextEntry::MonoVar(quant_var.clone());
                ctx.add_entry(ctx_mono_var.clone());

                let theta = self.solve_subtype(ctx, &left_subtype, &body)?;

                Ok(theta.until(&ctx_mono_var))
            }
            (Ty::TyExVar(evar), ty) => {
                trace_rule("<: instantiateL");
                if ty.occurs_check(evar.clone()) {
                    return Err(format!("occurs check error instantiateL"));
                };
                self.instantiate(ctx, ty_a, ty_b)
            }
            (ty, Ty::TyExVar(evar)) => {
                trace_rule("<: instantiateR");
                if ty.occurs_check(evar.clone()) {
                    return Err(format!("occurs check error instantiateR"));
                };
                self.instantiate(ctx, ty_b, ty_a)
            }
            (lhs, rhs) => Err(format!("unknown subtyping relation {:?} <: {:?}", lhs, rhs)),
        }
    }

    fn instantiate(&mut self, ctx: &mut Context, ty_a: &Ty, ty_b: &Ty) -> Result<Context, String> {
        match (ty_a, ty_b) {
            (Ty::TyExVar(evar_a), Ty::TyExVar(evar_b)) => {
                trace_rule("instLReach / instRReach");
                let evar_a_entry = &ContextEntry::ExVar(evar_a.clone());
                let evar_b_entry = &ContextEntry::ExVar(evar_b.clone());
                if !ctx.before(evar_a_entry, evar_b_entry) {
                    return Err(format!(
                        "evar {} has to appear before evar {}",
                        evar_a.clone(),
                        evar_b.clone()
                    ));
                }

                let gamma = ctx.replace(
                    evar_b_entry,
                    &ContextEntry::SolvedEVar {
                        lhs: evar_b.clone(),
                        rhs: Ty::TyExVar(evar_a.clone()),
                    },
                );

                Ok(gamma.clone())
            }
            (Ty::TyExVar(evar), mono_ty)
                if mono_ty.is_mono() && !matches!(mono_ty, Ty::TyExVar(_)) =>
            {
                trace_rule("instLSolve");
                let (gamma_l, _, gamma_r) = ctx.split_2(&ContextEntry::ExVar(evar.clone()));
                let solution = ContextEntry::SolvedEVar {
                    lhs: evar.clone(),
                    rhs: mono_ty.clone(),
                };
                let mut gamma = gamma_l.clone();
                gamma.push(solution);
                gamma.extend(gamma_r);
                Ok(Context { entries: gamma })
            }
            (mono_ty, Ty::TyExVar(evar))
                if mono_ty.is_mono() && !matches!(mono_ty, Ty::TyExVar(_)) =>
            {
                trace_rule("instRSolve");
                let (gamma_l, _, gamma_r) = ctx.split_2(&ContextEntry::ExVar(evar.clone()));
                let solution = ContextEntry::SolvedEVar {
                    lhs: evar.clone(),
                    rhs: mono_ty.clone(),
                };
                let mut gamma = gamma_l.clone();
                gamma.push(solution);
                gamma.extend(gamma_r);

                Ok(Context { entries: gamma })
            }

            (Ty::TyExVar(_), Ty::TyForAll(quantifier, body)) => {
                trace_rule("InstLAllR");
                let quant_ctx_entry = ContextEntry::MonoVar(quantifier.clone());
                ctx.add_entry(quant_ctx_entry.clone());
                let delta = self.instantiate(ctx, ty_a, body)?.until(&quant_ctx_entry);

                Ok(delta)
            }
            (Ty::TyForAll(quantifier, body), Ty::TyExVar(_)) => {
                trace_rule("instRAllL");
                let beta_var = self.make_fresh_evar();
                let beta_ctx_evar = ContextEntry::ExVar(beta_var.clone());
                let beta_ctx_marker = ContextEntry::Marker {
                    var_name: beta_var.clone(),
                };

                ctx.add_entry(beta_ctx_marker.clone());
                ctx.add_entry(beta_ctx_evar);

                let delta = self.instantiate(
                    ctx,
                    &body.substitute(quantifier, &Ty::TyExVar(beta_var)),
                    ty_a,
                )?;

                Ok(delta.until(&beta_ctx_marker))
            }
            (Ty::TyExVar(evar), Ty::TyArr(b1, b2)) => {
                trace_rule("instLArr");
                let (mut gamma_l, _, gamma_r) = ctx.split_2(&ContextEntry::ExVar(evar.clone()));
                let a2_ex_var = self.make_fresh_evar();
                let a1_ex_var = self.make_fresh_evar();
                let solved_evar = ContextEntry::SolvedEVar {
                    lhs: evar.clone(),
                    rhs: Ty::make_arr(
                        Ty::TyExVar(a1_ex_var.clone()),
                        Ty::TyExVar(a2_ex_var.clone()),
                    ),
                };

                gamma_l.push(ContextEntry::ExVar(a2_ex_var.clone()));
                gamma_l.push(ContextEntry::ExVar(a1_ex_var.clone()));
                gamma_l.push(solved_evar);

                let mut new_ctx = Context::from_2(gamma_l, gamma_r);
                let theta = self.instantiate(&mut new_ctx, b1, &Ty::TyExVar(a1_ex_var.clone()))?;
                let delta = self.instantiate(
                    &mut theta.clone(),
                    &Ty::TyExVar(a2_ex_var.clone()),
                    &apply_context(&theta, b2),
                )?;
                Ok(delta)
            }
            (Ty::TyArr(b1, b2), Ty::TyExVar(evar)) => {
                trace_rule("instRArr");
                let (mut gamma_l, _, gamma_r) = ctx.split_2(&ContextEntry::ExVar(evar.clone()));
                let a2_ex_var = self.make_fresh_evar();
                let a1_ex_var = self.make_fresh_evar();
                let solved_evar = ContextEntry::SolvedEVar {
                    lhs: evar.clone(),
                    rhs: Ty::make_arr(
                        Ty::TyExVar(a1_ex_var.clone()),
                        Ty::TyExVar(a2_ex_var.clone()),
                    ),
                };

                gamma_l.push(ContextEntry::ExVar(a2_ex_var.clone()));
                gamma_l.push(ContextEntry::ExVar(a1_ex_var.clone()));
                gamma_l.push(solved_evar);

                let mut new_ctx = Context::from_2(gamma_l, gamma_r);
                let theta = self.instantiate(&mut new_ctx, &Ty::TyExVar(a1_ex_var.clone()), b1)?;
                let delta = self.instantiate(
                    &mut theta.clone(),
                    &apply_context(&theta, b2),
                    &Ty::TyExVar(a2_ex_var.clone()),
                )?;
                Ok(delta)
            }

            _ => todo!("unhandled instantiation case"),
        }
    }

    fn solve_checking(
        &mut self,
        ctx: &mut Context,
        expr: &CoreExpr,
        check_against: &Ty,
    ) -> Result<InferenceResult, String> {
        match (expr, check_against) {
            (CoreExpr::Unit, Ty::TyUnit) => Ok(InferenceResult {
                // 1I
                expr: expr.clone(),
                expr_ty: check_against.clone(),
                ctx: ctx.clone(),
            }),
            (CoreExpr::Abs(arg, body), Ty::TyArr(a1, a2)) => {
                trace_rule("->I");
                let typed_var_entry = ContextEntry::TypedVar {
                    var_name: arg.clone(),
                    ty: *a1.clone(),
                };
                ctx.add_entry(typed_var_entry.clone());
                let check_result = self.solve_checking(ctx, body, &a2.clone())?;
                Ok(InferenceResult {
                    expr: expr.clone(),
                    expr_ty: check_against.clone(),
                    ctx: check_result.ctx.until(&typed_var_entry),
                })
            }
            (_, Ty::TyForAll(quantifier, body)) => {
                trace_rule("forAllI");
                ctx.add_entry(ContextEntry::MonoVar(quantifier.clone()));

                let check_result = self.solve_checking(ctx, expr, body)?;

                Ok(InferenceResult {
                    expr: expr.clone(),
                    expr_ty: check_result.expr_ty,
                    ctx: check_result
                        .ctx
                        .until(&ContextEntry::MonoVar(quantifier.clone())),
                })
            }
            (_, _) => {
                trace_rule("Sub");
                let infer_result = self.solve_inference(ctx, expr)?;
                let subtype_result = self.solve_subtype(
                    &mut infer_result.ctx.clone(),
                    &apply_context(&infer_result.ctx, &infer_result.expr_ty),
                    &apply_context(&infer_result.ctx, check_against),
                )?;

                Ok(InferenceResult {
                    expr: expr.clone(),
                    expr_ty: check_against.clone(),
                    ctx: subtype_result,
                })
            }
        }
    }

    fn solve_app_inference(
        &mut self,
        ctx: &mut Context,
        lhs_type: &Ty,
        expr: &CoreExpr,
    ) -> Result<InferenceResult, String> {
        match (lhs_type, expr) {
            (Ty::TyExVar(evar), _) => {
                trace_rule("existApp");
                let (mut gamma_l, _, gamma_r) = ctx.split_2(&ContextEntry::ExVar(evar.clone()));
                let alpha_2 = self.make_fresh_evar();
                let alpha_1 = self.make_fresh_evar();
                gamma_l.push(ContextEntry::ExVar(alpha_2.clone()));
                gamma_l.push(ContextEntry::ExVar(alpha_1.clone()));
                gamma_l.push(ContextEntry::SolvedEVar {
                    lhs: evar.clone(),
                    rhs: Ty::make_arr(Ty::TyExVar(alpha_1.clone()), Ty::TyExVar(alpha_2.clone())),
                });

                let mut merged_ctx = Context::from_2(gamma_l, gamma_r);
                let delta_result =
                    self.solve_checking(&mut merged_ctx, expr, &Ty::TyExVar(alpha_1.clone()))?;

                Ok(InferenceResult {
                    expr: expr.clone(),
                    expr_ty: Ty::TyExVar(alpha_2.clone()),
                    ctx: delta_result.ctx,
                })
            }
            (Ty::TyForAll(quantifier, body), _) => {
                trace_rule("forAllApp");
                let alpha = self.make_fresh_evar();
                let alpha_ctx_evar = ContextEntry::ExVar(alpha.clone());
                ctx.add_entry(alpha_ctx_evar);

                let inf_left_type = self.solve_app_inference(
                    ctx,
                    &body.substitute(quantifier, &Ty::TyExVar(alpha.clone())),
                    expr,
                )?;

                Ok(InferenceResult {
                    expr: expr.clone(),
                    expr_ty: inf_left_type.expr_ty,
                    ctx: inf_left_type.ctx,
                })
            }
            (Ty::TyArr(a1, a2), _) => {
                trace_rule("arrApp");
                let checked_result = self.solve_checking(ctx, expr, a1)?;
                Ok(InferenceResult {
                    expr: expr.clone(),
                    expr_ty: *a2.clone(),
                    ctx: checked_result.ctx,
                })
            }

            _ => todo!("unacceptable app inference case"),
        }
    }

    fn solve_inference(
        &mut self,
        ctx: &mut Context,
        expr: &CoreExpr,
    ) -> Result<InferenceResult, String> {
        match expr {
            CoreExpr::Unit => Ok(InferenceResult {
                expr: expr.clone(),
                expr_ty: Ty::TyUnit,
                ctx: ctx.clone(),
            }),
            CoreExpr::Var(name) => {
                trace_rule("Var");
                if let Some(ContextEntry::TypedVar { var_name: _, ty }) =
                    ctx.get_typed_var(name.clone())
                {
                    return Ok(InferenceResult {
                        expr: expr.clone(),
                        expr_ty: ty.clone(),
                        ctx: ctx.clone(),
                    });
                }

                Err(format!("variable {} not in scope", name))
            }
            CoreExpr::Abs(arg, body) => {
                trace_rule("I=>");
                let alpha_ctx_evar = self.make_fresh_evar();
                let beta_ctx_evar = self.make_fresh_evar();
                let arg_ctx_evar = ContextEntry::TypedVar {
                    var_name: arg.clone(),
                    ty: Ty::TyExVar(alpha_ctx_evar.clone()),
                };

                ctx.add_entry(ContextEntry::ExVar(alpha_ctx_evar.clone()));
                ctx.add_entry(ContextEntry::ExVar(beta_ctx_evar.clone()));
                ctx.add_entry(arg_ctx_evar.clone());

                let checked =
                    self.solve_checking(ctx, body, &Ty::TyExVar(beta_ctx_evar.clone()))?;

                Ok(InferenceResult {
                    expr: expr.clone(),
                    expr_ty: Ty::make_arr(
                        Ty::TyExVar(alpha_ctx_evar.clone()),
                        Ty::TyExVar(beta_ctx_evar.clone()),
                    ),
                    ctx: checked.ctx.until(&arg_ctx_evar.clone()),
                })
            }
            CoreExpr::App(left, right) => {
                trace_rule("->E");
                let mut inf_left = self.solve_inference(ctx, left)?;
                let ctx_applied_to_inf = apply_context(&inf_left.ctx, &inf_left.expr_ty);
                let inf_app = self.solve_app_inference(
                    &mut inf_left.ctx,
                    &ctx_applied_to_inf,
                    right,
                )?;

                Ok(inf_app)
            }
            _ => todo!(),
        }
    }
}

fn main() {
    let f_var = Box::new(CoreExpr::Var("f".to_string()));
    let g_var = Box::new(CoreExpr::Var("g".to_string()));
    let f1 = CoreExpr::Abs("f".to_string(), Box::new(CoreExpr::App(f_var, g_var)));
    let f2 = CoreExpr::Abs("g".to_string(), Box::new(f1.clone()));

    let app = CoreExpr::App(Box::new(f2.clone()), Box::new(CoreExpr::Unit));

    let mut inference = TypeInference::new();
    let mut ctx = Context::new();

    let solved = inference.solve_inference(&mut ctx, &app).unwrap();
    let result_type = solved.expr_ty.clone();

    println!("{:?}", apply_context(&solved.ctx, &result_type));

}
