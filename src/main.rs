use std::ops::Index;

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
                Self::make_forall(quantifier.clone(), body.substitute(var_name, with_type))
            }
            _ => panic!("subst failed"),
        }
    }
}

#[derive(Clone, Debug)]
enum CoreExpr {
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
        let found_ix = self.entries.iter().position(|ent| ent == entry);
        match found_ix {
            Some(ix) => {
                let mut new_ctx = self.entries.clone();
                let _split_entries = new_ctx.split_off(ix);

                Context { entries: new_ctx }
            }
            None => panic!("error context until"),
        }
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

            if ctx.has_evar(ex_var_name.clone()) {
                return ty.clone();
            }
            let err = format!(
                "evar {} is missing during context application",
                *ex_var_name
            );
            panic!("{}", err)
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

impl TypeInference {
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
                let theta_ctx = self.solve_subtype(ctx, b1, a1)?;
                let delta_ctx = self.solve_subtype(
                    ctx,
                    &apply_context(&theta_ctx, &a2),
                    &apply_context(&theta_ctx, &b2),
                )?;

                Ok(delta_ctx)
            }
            (Ty::TyForAll(quant_var, body), right_subtype) => {
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
                let ctx_mono_var = ContextEntry::MonoVar(quant_var.clone());
                ctx.add_entry(ctx_mono_var.clone());

                let theta = self.solve_subtype(ctx, &left_subtype, &body)?;

                Ok(theta.until(&ctx_mono_var))
            }
            (Ty::TyExVar(evar), ty) => {
                todo!("instantiateL")
            }
            (ty, Ty::TyExVar(evar)) => {
                todo!("intantiateR")
            }
            (lhs, rhs) => Err(format!("unknown subtyping relation {:?} <: {:?}", lhs, rhs)),
        }
    }

    fn instantiate(&mut self, ctx: &mut Context, ty_a: &Ty, ty_b: &Ty) -> Result<Context, String> {
        match (ty_a, ty_b) {
            (Ty::TyExVar(evar), mono_ty) | (mono_ty, Ty::TyExVar(evar)) if mono_ty.is_mono() => {
                // instLSolve / instRSolve
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
            (Ty::TyExVar(evar_a), Ty::TyExVar(evar_b)) => {
                // instLReach / instRReach
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
            (Ty::TyExVar(_), Ty::TyForAll(quantifier, body)) => {
                // InstLAllR
                let quant_ctx_entry = ContextEntry::MonoVar(quantifier.clone());
                let delta = self.instantiate(ctx, ty_a, body)?.until(&quant_ctx_entry);

                Ok(delta)
            }
            (Ty::TyForAll(quantifier, body), Ty::TyExVar(_)) => {
                // instRAllL
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
                // instLArr
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
                    &mut new_ctx,
                    &Ty::TyExVar(a2_ex_var.clone()),
                    &apply_context(&theta, b2),
                )?;
                Ok(delta)
            }
            (Ty::TyArr(b1, b2), Ty::TyExVar(evar)) => {
                // instRArr
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
                    &mut new_ctx,
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
        todo!()
    }

    fn solve_app_inference(
        &mut self,
        ctx: &mut Context,
        lhs_type: &Ty,
        expr: &CoreExpr,
    ) -> Result<InferenceResult, String> {
        match (lhs_type, expr) {
            (Ty::TyExVar(evar), _) => {
                // existApp
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
                // forAllApp
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
                // arrApp
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
            CoreExpr::App(left, right) => {
                let inf_left = self.solve_inference(ctx, left)?;
                let ctx_applied_to_inf = apply_context(&inf_left.ctx, &inf_left.expr_ty);
                let inf_app = self.solve_app_inference(ctx, &ctx_applied_to_inf, right)?;

                Ok(inf_app)
            }
            _ => todo!(),
        }
    }
}

fn main() {
    println!("Hello, world!");

    let mut ctx = Context::new();
    ctx.add_entry(ContextEntry::ExVar("a".to_string()));
    ctx.add_entry(ContextEntry::MonoVar("b".to_string()));
    ctx.add_entry(ContextEntry::Marker {
        var_name: "ex1".to_string(),
    });
    ctx.add_entry(ContextEntry::MonoVar("c".to_string()));

    let new_ctx = ctx.split_2(&ContextEntry::Marker {
        var_name: "ex1".to_string(),
    });

    println!("{:?}", new_ctx);
}
