use std::collections::HashMap;
use std::rc::Rc;

use super::closure_ir::{CCCaseArm, CCExpr, CCFunction, CCGlobalDef, CCPattern, CCProgram};
use super::lower_ast::{IRCaseArm, IRExpr, IRGlobalDef, IRPattern, IRProgram};

#[derive(Debug, Clone, Copy)]
enum VisibleBinding {
    Local,
    Inherited(usize),
}

#[derive(Debug, Clone, Copy)]
struct KnownGlobalFunc {
    func_id: usize,
    arity: usize,
}

pub fn lower_to_closure_ir(program: &IRProgram) -> CCProgram {
    ClosureLowerer::new(program).lower_program(program)
}

struct ClosureLowerer {
    global_indices: HashMap<String, usize>,
    known_global_funcs: Vec<Option<KnownGlobalFunc>>,
    functions: Vec<Option<CCFunction>>,
    next_func_id: usize,
    next_lambda_serial: usize,
}

impl ClosureLowerer {
    fn new(program: &IRProgram) -> Self {
        let global_indices = program
            .globals
            .iter()
            .enumerate()
            .map(|(idx, global)| (global.name.clone(), idx))
            .collect::<HashMap<_, _>>();

        let mut known_global_funcs = vec![None; program.globals.len()];
        let mut functions = Vec::new();
        let mut next_func_id = 0;

        // Reserve function ids for top-level lambda chains up front so that
        // saturated global calls can target them directly during lowering.
        for (global_idx, global) in program.globals.iter().enumerate() {
            let (arity, _) = collect_lambda_chain(&global.body);
            if arity > 0 {
                let func_id = next_func_id;
                next_func_id += 1;
                if functions.len() <= func_id {
                    functions.resize_with(func_id + 1, || None);
                }
                known_global_funcs[global_idx] = Some(KnownGlobalFunc { func_id, arity });
            }
        }

        Self {
            global_indices,
            known_global_funcs,
            functions,
            next_func_id,
            next_lambda_serial: 0,
        }
    }

    fn lower_program(mut self, program: &IRProgram) -> CCProgram {
        let globals = program
            .globals
            .iter()
            .enumerate()
            .map(|(idx, global)| self.lower_global(idx, global))
            .collect();

        let functions = self
            .functions
            .into_iter()
            .enumerate()
            .map(|(id, func)| {
                func.unwrap_or_else(|| panic!("missing closure-converted function for id {}", id))
            })
            .collect();

        CCProgram {
            globals,
            functions,
            constructor_arities: program.constructor_arities.clone(),
        }
    }

    fn lower_global(&mut self, global_index: usize, global: &IRGlobalDef) -> CCGlobalDef {
        if let Some(info) = self.known_global_funcs[global_index] {
            let (_, lambda_body) = collect_lambda_chain(&global.body);

            let (lowered_body, capture_sources, capture_count) = {
                let mut child = ExprLowerer::new_lambda(self, global.name.clone(), 0, info.arity);

                let lowered_body = child.lower_expr(&lambda_body);
                let capture_sources = child.capture_sources_in_slot_order();
                let capture_count = child.capture_slots.len();

                (lowered_body, capture_sources, capture_count)
            };

            let captures: Vec<Rc<CCExpr>> = capture_sources
                .into_iter()
                .map(|visible_pos| {
                    panic!(
                        "top-level function '{}' unexpectedly captured visible slot {}",
                        global.name, visible_pos
                    )
                })
                .collect();

            assert!(
                captures.is_empty(),
                "top-level function '{}' unexpectedly captured locals",
                global.name
            );

            self.push_function(CCFunction {
                id: info.func_id,
                name: global.name.clone(),
                arity: info.arity,
                capture_count,
                body: lowered_body,
            });

            return CCGlobalDef {
                name: global.name.clone(),
                body: Rc::new(CCExpr::MakeClosure {
                    func: info.func_id,
                    captures: vec![],
                }),
            };
        }

        let mut lowerer = ExprLowerer::new_root(self, global.name.clone());
        let body = lowerer.lower_expr(&global.body);

        CCGlobalDef {
            name: global.name.clone(),
            body,
        }
    }

    fn fresh_func_id(&mut self) -> usize {
        let id = self.next_func_id;
        self.next_func_id += 1;

        if self.functions.len() <= id {
            self.functions.resize_with(id + 1, || None);
        }

        id
    }

    fn fresh_lambda_name(&mut self, owner_name: &str) -> String {
        let serial = self.next_lambda_serial;
        self.next_lambda_serial += 1;
        format!("{owner_name}$lambda{serial}")
    }

    fn global_index_by_name(&self, name: &str) -> usize {
        self.global_indices
            .get(name)
            .copied()
            .unwrap_or_else(|| panic!("unknown global '{}' during closure lowering", name))
    }

    fn known_global_func(&self, global_index: usize) -> Option<KnownGlobalFunc> {
        self.known_global_funcs.get(global_index).copied().flatten()
    }

    fn push_function(&mut self, function: CCFunction) {
        let id = function.id;

        if self.functions.len() <= id {
            self.functions.resize_with(id + 1, || None);
        }

        assert!(
            self.functions[id].is_none(),
            "duplicate closure-converted function id {}",
            id
        );

        self.functions[id] = Some(function);
    }
}

struct ExprLowerer<'a> {
    owner_name: String,
    state: &'a mut ClosureLowerer,
    visible: Vec<VisibleBinding>,
    capture_slots: HashMap<usize, usize>,
}

impl<'a> ExprLowerer<'a> {
    fn new_root(state: &'a mut ClosureLowerer, owner_name: String) -> Self {
        Self {
            owner_name,
            state,
            visible: Vec::new(),
            capture_slots: HashMap::new(),
        }
    }

    fn new_lambda(
        state: &'a mut ClosureLowerer,
        owner_name: String,
        parent_visible_len: usize,
        arity: usize,
    ) -> Self {
        let mut visible = Vec::with_capacity(parent_visible_len + arity);

        for parent_pos in 0..parent_visible_len {
            visible.push(VisibleBinding::Inherited(parent_pos));
        }

        for _ in 0..arity {
            visible.push(VisibleBinding::Local);
        }

        Self {
            owner_name,
            state,
            visible,
            capture_slots: HashMap::new(),
        }
    }

    fn lower_expr(&mut self, expr: &Rc<IRExpr>) -> Rc<CCExpr> {
        match expr.as_ref() {
            IRExpr::Unit => Rc::new(CCExpr::Unit),
            IRExpr::Int(n) => Rc::new(CCExpr::Int(*n)),
            IRExpr::BVar(index) => self.lower_bvar(*index),
            IRExpr::FVar(index) => Rc::new(CCExpr::Global(*index)),
            IRExpr::Con { tag, arity } => Rc::new(CCExpr::CtorFn {
                tag: *tag,
                arity: *arity,
            }),
            IRExpr::UnaryOp(op, rhs) => Rc::new(CCExpr::UnaryOp(op.clone(), self.lower_expr(rhs))),
            IRExpr::BinaryOp(op, lhs, rhs) => Rc::new(CCExpr::BinaryOp(
                op.clone(),
                self.lower_expr(lhs),
                self.lower_expr(rhs),
            )),
            IRExpr::Abs(_) => self.lower_lambda(expr),
            IRExpr::App(_, _) => self.lower_application(expr),
            IRExpr::Let(binder, body) => {
                let lowered_binder = self.lower_expr(binder);

                self.push_local();
                let lowered_body = self.lower_expr(body);
                self.pop_local();

                Rc::new(CCExpr::Let(lowered_binder, lowered_body))
            }
            IRExpr::Case { scrutinee, arms } => {
                let lowered_scrutinee = self.lower_expr(scrutinee);
                let lowered_arms = arms.iter().map(|arm| self.lower_case_arm(arm)).collect();

                Rc::new(CCExpr::Case {
                    scrutinee: lowered_scrutinee,
                    arms: lowered_arms,
                })
            }
            IRExpr::IfThenElse {
                cond,
                then_br,
                else_br,
            } => Rc::new(CCExpr::IfThenElse {
                cond: self.lower_expr(cond),
                then_br: self.lower_expr(then_br),
                else_br: self.lower_expr(else_br),
            }),
        }
    }

    fn lower_lambda(&mut self, lambda_expr: &Rc<IRExpr>) -> Rc<CCExpr> {
        let (arity, lambda_body) = collect_lambda_chain(lambda_expr);
        let func_id = self.state.fresh_func_id();
        let func_name = self.state.fresh_lambda_name(&self.owner_name);
        let parent_visible_len = self.visible.len();

        let (lowered_body, capture_sources, capture_count) = {
            let mut child =
                ExprLowerer::new_lambda(self.state, func_name.clone(), parent_visible_len, arity);

            let lowered_body = child.lower_expr(&lambda_body);
            let capture_sources = child.capture_sources_in_slot_order();
            let capture_count = child.capture_slots.len();

            (lowered_body, capture_sources, capture_count)
        };

        self.state.push_function(CCFunction {
            id: func_id,
            name: func_name,
            arity,
            capture_count,
            body: lowered_body,
        });

        let captures = capture_sources
            .into_iter()
            .map(|parent_visible_pos| self.expr_for_visible_position(parent_visible_pos))
            .collect();

        Rc::new(CCExpr::MakeClosure {
            func: func_id,
            captures,
        })
    }

    fn lower_application(&mut self, expr: &Rc<IRExpr>) -> Rc<CCExpr> {
        let (head, args) = collect_application_spine(expr);

        if let IRExpr::Con { tag, arity } = head.as_ref() {
            if args.len() == *arity as usize {
                let lowered_fields = args.iter().map(|arg| self.lower_expr(arg)).collect();

                return Rc::new(CCExpr::Pack {
                    tag: *tag,
                    arity: *arity,
                    fields: lowered_fields,
                });
            }
        }

          if let IRExpr::FVar(global_index) = head.as_ref() {
            if let Some(info) = self.state.known_global_func(*global_index) {
                if args.len() == info.arity {
                    let lowered_args = args.iter().map(|arg| self.lower_expr(arg)).collect();

                    return Rc::new(CCExpr::CallKnown {
                        func: info.func_id,
                        args: lowered_args,
                    });
                }
            }
        }

        let lowered_head = self.lower_expr(&head);
        let lowered_args = args.iter().map(|arg| self.lower_expr(arg)).collect();

        Rc::new(CCExpr::Call {
            callee: lowered_head,
            args: lowered_args,
        })
    }

    fn lower_case_arm(&mut self, arm: &IRCaseArm) -> CCCaseArm {
        let pattern = self.lower_pattern(&arm.pattern);

        self.push_pattern_binders(&arm.pattern);
        let body = self.lower_expr(&arm.body);
        self.pop_pattern_binders(&arm.pattern);

        CCCaseArm { pattern, body }
    }

    fn lower_pattern(&self, pattern: &IRPattern) -> CCPattern {
        match pattern {
            IRPattern::Var => CCPattern::Var,
            IRPattern::Wild => CCPattern::Wild,
            IRPattern::Con(tag, sub_patterns) => CCPattern::Con(
                *tag,
                sub_patterns
                    .iter()
                    .map(|sub_pattern| self.lower_pattern(sub_pattern))
                    .collect(),
            ),
        }
    }

    fn lower_bvar(&mut self, index: usize) -> Rc<CCExpr> {
        let visible_pos = self
            .visible
            .len()
            .checked_sub(index + 1)
            .unwrap_or_else(|| panic!("bad de Bruijn index {} during closure lowering", index));

        match self.visible[visible_pos] {
            VisibleBinding::Local => Rc::new(CCExpr::Local(
                self.local_debruijn_from_visible_position(visible_pos),
            )),
            VisibleBinding::Inherited(parent_visible_pos) => {
                let capture_slot = self.capture_slot_for(parent_visible_pos);
                Rc::new(CCExpr::Capture(capture_slot))
            }
        }
    }

    fn expr_for_visible_position(&mut self, visible_pos: usize) -> Rc<CCExpr> {
        match self.visible[visible_pos] {
            VisibleBinding::Local => Rc::new(CCExpr::Local(
                self.local_debruijn_from_visible_position(visible_pos),
            )),
            VisibleBinding::Inherited(parent_visible_pos) => {
                let capture_slot = self.capture_slot_for(parent_visible_pos);
                Rc::new(CCExpr::Capture(capture_slot))
            }
        }
    }

    fn local_debruijn_from_visible_position(&self, visible_pos: usize) -> usize {
        self.visible[visible_pos + 1..]
            .iter()
            .filter(|binding| matches!(binding, VisibleBinding::Local))
            .count()
    }

    fn capture_slot_for(&mut self, parent_visible_pos: usize) -> usize {
        if let Some(&slot) = self.capture_slots.get(&parent_visible_pos) {
            return slot;
        }

        let slot = self.capture_slots.len();
        self.capture_slots.insert(parent_visible_pos, slot);
        slot
    }

    fn capture_sources_in_slot_order(&self) -> Vec<usize> {
        let mut out = vec![0; self.capture_slots.len()];

        for (&parent_visible_pos, &slot) in &self.capture_slots {
            out[slot] = parent_visible_pos;
        }

        out
    }

    fn push_local(&mut self) {
        self.visible.push(VisibleBinding::Local);
    }

    fn pop_local(&mut self) {
        match self.visible.pop() {
            Some(VisibleBinding::Local) => {}
            Some(VisibleBinding::Inherited(_)) => {
                panic!("internal error: attempted to pop inherited binding as local")
            }
            None => panic!("internal error: attempted to pop local from empty visible stack"),
        }
    }

    fn push_pattern_binders(&mut self, pattern: &IRPattern) {
        match pattern {
            IRPattern::Var => self.push_local(),
            IRPattern::Wild => {}
            IRPattern::Con(_, sub_patterns) => {
                for sub_pattern in sub_patterns {
                    self.push_pattern_binders(sub_pattern);
                }
            }
        }
    }

    fn pop_pattern_binders(&mut self, pattern: &IRPattern) {
        match pattern {
            IRPattern::Var => self.pop_local(),
            IRPattern::Wild => {}
            IRPattern::Con(_, sub_patterns) => {
                for sub_pattern in sub_patterns.iter().rev() {
                    self.pop_pattern_binders(sub_pattern);
                }
            }
        }
    }
}

fn collect_application_spine(expr: &Rc<IRExpr>) -> (Rc<IRExpr>, Vec<Rc<IRExpr>>) {
    let mut args = Vec::new();
    let mut head = expr.clone();

    loop {
        match head.as_ref() {
            IRExpr::App(func, arg) => {
                args.push(arg.clone());
                head = func.clone();
            }
            _ => {
                args.reverse();
                return (head, args);
            }
        }
    }
}

fn collect_lambda_chain(expr: &Rc<IRExpr>) -> (usize, Rc<IRExpr>) {
    let mut arity = 0;
    let mut body = expr.clone();

    loop {
        match body.as_ref() {
            IRExpr::Abs(inner) => {
                arity += 1;
                body = inner.clone();
            }
            _ => return (arity, body),
        }
    }
}
