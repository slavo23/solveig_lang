use std::collections::HashMap;
use std::rc::Rc;

use solveig_ast::ast::{
    CaseArm, CtorId, CtorRuntimeInfo, Literal, Pattern, PatternNode, ResolvedModule, TermExpr,
    TermExprNode, VarRef, symbol_display_name, var_display_name,
};

use super::lowered_ast::{IRCaseArm, IRExpr, IRGlobalDef, IRPattern, IRProgram};

struct LowerCtx {
    scope: Vec<String>,
    globals: HashMap<String, usize>,
    constructors: HashMap<CtorId, LowerConstructorInfo>,
}

#[derive(Clone, Debug)]
struct LowerConstructorInfo {
    name: String,
    short_name: String,
    tag: u16,
    arity: u16,
}

impl LowerCtx {
    fn resolve_var(&self, var: &VarRef) -> Rc<IRExpr> {
        let name = var_display_name(var);

        for (depth, bound_name) in self.scope.iter().rev().enumerate() {
            if bound_name == &name {
                return Rc::new(IRExpr::BVar(depth));
            }
        }
        if let Some(&global_index) = self.globals.get(&name) {
            return Rc::new(IRExpr::FVar(global_index));
        }
        if let Some(info) = self.constructor_named(&name) {
            return Rc::new(IRExpr::Con {
                tag: info.tag,
                arity: info.arity,
            });
        }
        panic!("unbound variable during lowering: {}", name);
    }

    fn constructor_named(&self, name: &str) -> Option<&LowerConstructorInfo> {
        self.constructors
            .values()
            .find(|info| info.name == name || info.short_name == name)
    }

    fn constructor_name_for_error(&self, id: CtorId) -> String {
        self.constructors
            .get(&id)
            .map(|info| info.name.clone())
            .unwrap_or_else(|| format!("C{}", id.0))
    }

    fn push(&mut self, name: &str) {
        self.scope.push(name.to_string());
    }

    fn pop(&mut self) {
        self.scope.pop();
    }

    fn push_pattern_binders<A>(&mut self, pattern: &PatternNode<A>) {
        match &pattern.node {
            Pattern::Var(name) => self.push(name),
            Pattern::Wild => {}
            Pattern::Lit(_) => {}
            Pattern::Con(_, sub_patterns) => {
                for sub_pattern in sub_patterns {
                    self.push_pattern_binders(sub_pattern);
                }
            }
        }
    }

    fn pop_pattern_binders<A>(&mut self, pattern: &PatternNode<A>) {
        match &pattern.node {
            Pattern::Var(_) => {
                self.pop();
            }
            Pattern::Lit(_) => {}
            Pattern::Wild => {}
            Pattern::Con(_, sub_patterns) => {
                for sub_pattern in sub_patterns.iter().rev() {
                    self.pop_pattern_binders(sub_pattern);
                }
            }
        }
    }
}

fn lower_expr<A, P>(ctx: &mut LowerCtx, expr: &TermExprNode<A, P>) -> Rc<IRExpr> {
    match &expr.node {
        TermExpr::Unit => Rc::new(IRExpr::Unit),
        TermExpr::Var(name) => ctx.resolve_var(name),
        TermExpr::Lit(Literal::Int(n)) => Rc::new(IRExpr::Int(*n)),
        TermExpr::Con(id) => {
            let info = ctx.constructors.get(id).unwrap_or_else(|| {
                panic!(
                    "unknown constructor during lowering: {}",
                    ctx.constructor_name_for_error(*id)
                )
            });
            Rc::new(IRExpr::Con {
                tag: info.tag,
                arity: info.arity,
            })
        }

        TermExpr::Abs(param, _, body) => {
            ctx.push(param);
            let lowered_body = lower_expr(ctx, body);
            ctx.pop();
            Rc::new(IRExpr::Abs(lowered_body))
        }

        TermExpr::App(func, arg) => {
            let lowered_func = lower_expr(ctx, func);
            let lowered_arg = lower_expr(ctx, arg);
            Rc::new(IRExpr::App(lowered_func, lowered_arg))
        }

        TermExpr::TyAbs(_, _, body) => lower_expr(ctx, body),
        TermExpr::TyApp(inner, _) => lower_expr(ctx, inner),
        TermExpr::Anno(inner, _) => lower_expr(ctx, inner),

        TermExpr::Case(scrutinee, arms) => {
            let lowered_scrutinee = lower_expr(ctx, scrutinee);
            let lowered_arms = arms
                .iter()
                .map(|arm_node| lower_case_arm(ctx, &arm_node.node))
                .collect();
            Rc::new(IRExpr::Case {
                scrutinee: lowered_scrutinee,
                arms: lowered_arms,
            })
        }

        TermExpr::IfThenElse(cond, then_branch, else_branch) => {
            let lowered_cond = lower_expr(ctx, cond);
            let lowered_then = lower_expr(ctx, then_branch);
            let lowered_else = lower_expr(ctx, else_branch);
            Rc::new(IRExpr::IfThenElse {
                cond: lowered_cond,
                then_br: lowered_then,
                else_br: lowered_else,
            })
        }
        TermExpr::BinaryOperator(op, lhs, rhs) => {
            let lowered_lhs = lower_expr(ctx, lhs);
            let lowered_rhs = lower_expr(ctx, rhs);
            Rc::new(IRExpr::BinaryOp(*op, lowered_lhs, lowered_rhs))
        }

        TermExpr::UnaryOperator(op, rhs) => {
            let lowered_rhs = lower_expr(ctx, rhs);
            Rc::new(IRExpr::UnaryOp(*op, lowered_rhs))
        }
        TermExpr::Let(name, binder, in_expr) => {
            let lowered_binder = lower_expr(ctx, binder);

            ctx.push(name);
            let lowered_in_expr = lower_expr(ctx, in_expr);
            ctx.pop();

            Rc::new(IRExpr::Let(lowered_binder, lowered_in_expr))
        }
        TermExpr::Hole => panic!("Program is incomplete and contains holes. Aborting compilation."),
    }
}

fn lower_case_arm<A, P>(ctx: &mut LowerCtx, arm: &CaseArm<A, P>) -> IRCaseArm {
    let ir_pattern = lower_pattern(ctx, &arm.pattern);
    ctx.push_pattern_binders(&arm.pattern);
    let lowered_body = lower_expr(ctx, &arm.expr);
    ctx.pop_pattern_binders(&arm.pattern);
    IRCaseArm {
        pattern: ir_pattern,
        body: lowered_body,
    }
}

fn lower_pattern<A>(ctx: &LowerCtx, pattern: &PatternNode<A>) -> IRPattern {
    match &pattern.node {
        Pattern::Var(_) => IRPattern::Var,
        Pattern::Wild => IRPattern::Wild,
        Pattern::Lit(lit_val) => IRPattern::Lit(lit_val.clone()),
        Pattern::Con(id, sub_patterns) => {
            let info = ctx.constructors.get(id).unwrap_or_else(|| {
                panic!(
                    "unknown constructor in pattern: {}",
                    ctx.constructor_name_for_error(*id)
                )
            });
            let sub_ir_patterns = sub_patterns
                .iter()
                .map(|sub_pattern| lower_pattern(ctx, sub_pattern))
                .collect();
            IRPattern::Con(info.tag, sub_ir_patterns)
        }
    }
}

fn build_constructor_table<A, P>(
    module: &ResolvedModule<A, P>,
) -> HashMap<CtorId, LowerConstructorInfo> {
    let mut table = HashMap::new();
    for data_def in &module.data_defs {
        for constructor_def in &data_def.constructors {
            let table_info = module
                .ctor_table
                .get(constructor_def.id)
                .unwrap_or_else(|| {
                    panic!(
                        "ICE: lowering constructor table missing CtorId {:?}",
                        constructor_def.id
                    )
                });
            let arity = constructor_def.fields.len() as u16;
            table.insert(
                constructor_def.id,
                LowerConstructorInfo {
                    name: symbol_display_name(table_info.name),
                    short_name: symbol_display_name(table_info.short_name),
                    tag: table_info.tag,
                    arity,
                },
            );
        }
    }
    table
}

pub fn lower_module<A, P>(module: &ResolvedModule<A, P>) -> IRProgram {
    let constructors = build_constructor_table(module);

    let mut globals: HashMap<String, usize> = HashMap::new();
    for (index, func_def) in module.func_defs.iter().enumerate() {
        globals.insert(func_def.name.clone(), index);
    }

    let mut ctx = LowerCtx {
        scope: Vec::new(),
        globals,
        constructors: constructors.clone(),
    };

    let ir_globals = module
        .func_defs
        .iter()
        .map(|func_def| {
            let lowered_body = lower_expr(&mut ctx, &func_def.body);
            IRGlobalDef {
                name: func_def.name.clone(),
                body: lowered_body,
            }
        })
        .collect();

    let constructor_arities = constructors
        .iter()
        .map(|(ctor, info)| CtorRuntimeInfo {
            ctor: *ctor,
            name: info.name.clone(),
            short_name: info.short_name.clone(),
            tag: info.tag,
            arity: info.arity,
        })
        .collect();

    IRProgram {
        globals: ir_globals,
        constructor_arities,
    }
}
