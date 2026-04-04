use std::collections::HashMap;
use std::rc::Rc;

use core::ast::{
    CaseArm, DataDefinition, Literal, Pattern, PatternNode, ResolvedModule, TermExpr, TermExprNode,
};

use super::lower_ast::{IRCaseArm, IRExpr, IRGlobalDef, IRPattern, IRProgram};

struct LowerCtx {
    scope: Vec<String>,
    globals: HashMap<String, usize>,
    constructors: HashMap<String, (u16, u16)>,
}

impl LowerCtx {
    fn resolve_var(&self, name: &str) -> Rc<IRExpr> {
        for (depth, bound_name) in self.scope.iter().rev().enumerate() {
            if bound_name == name {
                return Rc::new(IRExpr::BVar(depth));
            }
        }
        if let Some(&global_index) = self.globals.get(name) {
            return Rc::new(IRExpr::FVar(global_index));
        }
        if let Some(&(tag, arity)) = self.constructors.get(name) {
            return Rc::new(IRExpr::Con { tag, arity });
        }
        panic!("unbound variable during lowering: {}", name);
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
        TermExpr::Con(name) => {
            let &(tag, arity) = ctx
                .constructors
                .get(name)
                .unwrap_or_else(|| panic!("unknown constructor during lowering: {}", name));
            Rc::new(IRExpr::Con { tag, arity })
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
            Rc::new(IRExpr::BinaryOp(op.clone(), lowered_lhs, lowered_rhs))
        }

        TermExpr::UnaryOperator(op, rhs) => {
            let lowered_rhs = lower_expr(ctx, rhs);
            Rc::new(IRExpr::UnaryOp(op.clone(), lowered_rhs))
        }
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
        Pattern::Con(name, sub_patterns) => {
            let &(tag, _) = ctx
                .constructors
                .get(name)
                .unwrap_or_else(|| panic!("unknown constructor in pattern: {}", name));
            let sub_ir_patterns = sub_patterns
                .iter()
                .map(|sub_pattern| lower_pattern(ctx, sub_pattern))
                .collect();
            IRPattern::Con(tag, sub_ir_patterns)
        }
    }
}

fn build_constructor_table(data_defs: &[DataDefinition]) -> HashMap<String, (u16, u16)> {
    let mut table = HashMap::new();
    let mut next_tag: u16 = 0;
    for data_def in data_defs {
        for constructor_def in &data_def.constructors {
            let arity = constructor_def.fields.len() as u16;
            table.insert(constructor_def.name.clone(), (next_tag, arity));
            next_tag += 1;
        }
    }
    table
}

pub fn lower_module<A, P>(module: &ResolvedModule<A, P>) -> IRProgram {
    let constructors = build_constructor_table(&module.data_defs);

    let mut globals: HashMap<String, usize> = HashMap::new();
    for (index, func_def) in module.func_defs.iter().enumerate() {
        globals.insert(func_def.name.clone(), index);
    }

    for (name, &(_, _)) in &constructors {
        if !globals.contains_key(name) {
            // Don't shadow function names
        }
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
        .map(|(name, &(tag, arity))| (name.clone(), tag, arity))
        .collect();

    IRProgram {
        globals: ir_globals,
        constructor_arities,
    }
}
