//! Lowering from checked terms into VM-oriented core syntax.
//! This pass assigns globals, lowers constructors, and prepares IO primitives.

use solveig_base::{FileSpan, FxHashMap};
use std::rc::Rc;

use solveig_ast::ast::{
    BuiltinDefinition, CaseArm, ClassId, CtorId, CtorRuntimeInfo, DefId, Ev, EvVarId,
    InstanceDefinition, Literal, MatchArmNode, ModuleId, Names, Pattern, PatternNode,
    ResolvedModule, StrLitId, TermExpr, TermExprNode, VarRef, runtime_ctor_role,
};

use super::dicts::{DictLayout, DictLayouts, instance_global_name};
use super::lowered_ast::{IRExpr, IRGlobalDef, IRProgram};
use super::patterns::{LowerCaseArm, LowerPattern};

#[derive(Clone, Debug)]
enum Binding {
    Term(String),
    Evidence(EvVarId),
}

struct LowerCtx {
    names: Names,
    scope: Vec<Binding>,
    globals: FxHashMap<String, usize>,
    constructors: FxHashMap<CtorId, LowerConstructorInfo>,
    dicts: DictLayouts,
    string_constants: Vec<String>,
    string_indices: FxHashMap<StrLitId, u32>,
}

#[derive(Clone, Debug)]
struct LowerConstructorInfo {
    name: String,
    short_name: String,
    tag: u16,
    arity: u16,
}

#[derive(Clone, Debug)]
struct LowerMatchBinding {
    name: String,
    source: String,
}

#[derive(Clone, Debug)]
struct LowerMatchRow<A, P> {
    patterns: Vec<PatternNode<P>>,
    body: TermExprNode<A, P>,
    bindings: Vec<LowerMatchBinding>,
}

#[derive(Clone, Debug)]
enum LowerMatchHead {
    Constructor(CtorId),
    Literal(Literal),
}

impl LowerCtx {
    fn resolve_var(&self, var: &VarRef) -> Rc<IRExpr> {
        let name = self.names.var_display_name(var);
        if let Some(bound) = self.resolve_bound_term_name(&name) {
            return bound;
        }

        if let Some(&global_index) = self.globals.get(&name) {
            Rc::new(IRExpr::FVar(global_index))
        } else if let Some(info) = self.constructor_named(&name) {
            Rc::new(IRExpr::Con {
                tag: info.tag,
                arity: info.arity,
            })
        } else {
            solveig_base::ice!("unbound variable during lowering: {name}");
        }
    }

    fn resolve_bound_term_name(&self, name: &str) -> Option<Rc<IRExpr>> {
        for (depth, binding) in self.scope.iter().rev().enumerate() {
            if matches!(binding, Binding::Term(bound_name) if bound_name == name) {
                return Some(Rc::new(IRExpr::BVar(depth)));
            }
        }

        None
    }

    fn resolve_ev_var(&self, evvar: EvVarId) -> Rc<IRExpr> {
        for (depth, binding) in self.scope.iter().rev().enumerate() {
            if matches!(binding, Binding::Evidence(bound_evvar) if *bound_evvar == evvar) {
                return Rc::new(IRExpr::BVar(depth));
            }
        }

        solveig_base::ice!("unbound evidence variable during lowering: {evvar:?}");
    }

    fn constructor_named(&self, name: &str) -> Option<&LowerConstructorInfo> {
        self.constructors
            .values()
            .find(|info| info.name == name || info.short_name == name)
    }

    fn constructor_name_for_error(&self, id: CtorId) -> String {
        self.constructors
            .get(&id)
            .map_or_else(|| format!("C{}", id.0), |info| info.name.clone())
    }

    fn push_term(&mut self, name: &str) {
        self.scope.push(Binding::Term(name.to_string()));
    }

    fn push_evidence(&mut self, evvar: EvVarId) {
        self.scope.push(Binding::Evidence(evvar));
    }

    fn pop(&mut self) {
        self.scope.pop();
    }

    fn string_constant_index(&mut self, id: StrLitId) -> u32 {
        if let Some(index) = self.string_indices.get(&id) {
            return *index;
        }

        let index = u32::try_from(self.string_constants.len())
            .expect("ICE: string constant pool exceeded u32 capacity");
        self.string_constants
            .push(self.names.string_literal_value(id).to_string());
        self.string_indices.insert(id, index);
        index
    }

    fn push_pattern_binders<A>(&mut self, pattern: &PatternNode<A>) {
        match &pattern.node {
            Pattern::Var(name) => self.push_term(name),
            Pattern::Wild | Pattern::Lit(_) => {}
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
            Pattern::Lit(_) | Pattern::Wild => {}
            Pattern::Con(_, sub_patterns) => {
                for sub_pattern in sub_patterns.iter().rev() {
                    self.pop_pattern_binders(sub_pattern);
                }
            }
        }
    }
}

fn lower_expr<A: Clone, P: Clone, S>(
    ctx: &mut LowerCtx,
    expr: &TermExprNode<A, P>,
    source_span: &S,
) -> Rc<IRExpr>
where
    S: Fn(&A) -> Option<FileSpan>,
{
    let lowered = match &expr.node {
        TermExpr::Unit => Rc::new(IRExpr::Unit),
        TermExpr::Var(name) => ctx.resolve_var(name),
        TermExpr::Lit(lit) => match lit {
            Literal::Int(n) => Rc::new(IRExpr::Int(*n)),
            Literal::Float(n) => Rc::new(IRExpr::Float(*n)),
            Literal::Char(ch) => Rc::new(IRExpr::Char(*ch)),
            Literal::Str(id) => Rc::new(IRExpr::StrConst(ctx.string_constant_index(*id))),
        },
        TermExpr::Con(id) => {
            let info = ctx.constructors.get(id).unwrap_or_else(|| {
                solveig_base::ice!(
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
            ctx.push_term(param);
            let lowered_body = lower_expr(ctx, body, source_span);
            ctx.pop();
            Rc::new(IRExpr::Abs(lowered_body))
        }

        TermExpr::App(func, arg) => {
            let lowered_func = lower_expr(ctx, func, source_span);
            let lowered_arg = lower_expr(ctx, arg, source_span);
            Rc::new(IRExpr::App(lowered_func, lowered_arg))
        }

        TermExpr::TyAbs(_, _, body) => lower_expr(ctx, body, source_span),
        TermExpr::TyApp(inner, _) | TermExpr::Anno(inner, _) => lower_expr(ctx, inner, source_span),
        TermExpr::EvAbs(evvar, _, body) => {
            ctx.push_evidence(*evvar);
            let lowered_body = lower_expr(ctx, body, source_span);
            ctx.pop();
            Rc::new(IRExpr::Abs(lowered_body))
        }
        TermExpr::EvApp(inner, ev) => lower_ev_app(ctx, inner, ev, source_span),
        TermExpr::Case(scrutinee, arms) => {
            let lowered_scrutinee = lower_expr(ctx, scrutinee, source_span);
            let lowered_arms = arms
                .iter()
                .map(|arm_node| lower_case_arm(ctx, &arm_node.node, source_span))
                .collect();
            Rc::new(IRExpr::Case {
                scrutinee: lowered_scrutinee,
                arms: lowered_arms,
            })
        }
        TermExpr::Match(scrutinees, arms) => lower_match_expr(ctx, scrutinees, arms, source_span),

        TermExpr::IfThenElse(cond, then_branch, else_branch) => {
            let lowered_cond = lower_expr(ctx, cond, source_span);
            let lowered_then = lower_expr(ctx, then_branch, source_span);
            let lowered_else = lower_expr(ctx, else_branch, source_span);
            Rc::new(IRExpr::IfThenElse {
                cond: lowered_cond,
                then_br: lowered_then,
                else_br: lowered_else,
            })
        }
        TermExpr::Let(name, binder, in_expr) => {
            let lowered_binder = lower_expr(ctx, binder, source_span);

            ctx.push_term(name);
            let lowered_in_expr = lower_expr(ctx, in_expr, source_span);
            ctx.pop();

            Rc::new(IRExpr::Let(lowered_binder, lowered_in_expr))
        }
        TermExpr::Hole => solveig_base::ice!("driver allowed a typed hole to reach lowering"),
    };

    with_source(source_span(&expr.anno), lowered)
}

fn lower_match_expr<A: Clone, P: Clone, S>(
    ctx: &mut LowerCtx,
    scrutinees: &[TermExprNode<A, P>],
    arms: &[MatchArmNode<A, P>],
    source_span: &S,
) -> Rc<IRExpr>
where
    S: Fn(&A) -> Option<FileSpan>,
{
    if scrutinees.is_empty() {
        solveig_base::ice!("empty multi-pattern match reached lowering");
    }

    let columns = (0..scrutinees.len())
        .map(|index| format!("$matchValue{index}"))
        .collect::<Vec<_>>();
    let mut lowered_scrutinees = Vec::with_capacity(scrutinees.len());

    for (column, scrutinee) in columns.iter().zip(scrutinees) {
        lowered_scrutinees.push(lower_expr(ctx, scrutinee, source_span));
        ctx.push_term(column);
    }

    let rows = arms
        .iter()
        .map(|arm| LowerMatchRow {
            patterns: arm.node.patterns.clone(),
            body: arm.node.expr.clone(),
            bindings: Vec::new(),
        })
        .collect::<Vec<_>>();
    let mut next_field = 0usize;
    let mut body = lower_match_columns(ctx, &columns, rows, &mut next_field, source_span);

    for _ in &columns {
        ctx.pop();
    }

    for scrutinee in lowered_scrutinees.into_iter().rev() {
        body = Rc::new(IRExpr::Let(scrutinee, body));
    }

    body
}

fn lower_match_columns<A: Clone, P: Clone, S>(
    ctx: &mut LowerCtx,
    columns: &[String],
    rows: Vec<LowerMatchRow<A, P>>,
    next_field: &mut usize,
    source_span: &S,
) -> Rc<IRExpr>
where
    S: Fn(&A) -> Option<FileSpan>,
{
    if rows.is_empty() {
        solveig_base::ice!("empty match branch reached lowering");
    }

    let Some((column, rest_columns)) = columns.split_first() else {
        return lower_match_row_body(
            ctx,
            rows.first().expect("ICE: non-empty rows checked above"),
            source_span,
        );
    };

    let has_explicit_head = rows.iter().any(|row| {
        row.patterns
            .first()
            .is_some_and(|pattern| match_head(pattern).is_some())
    });

    if !has_explicit_head {
        let rows = rows
            .into_iter()
            .map(|row| lower_match_default_row(row, column))
            .collect::<Vec<_>>();
        return lower_match_columns(ctx, rest_columns, rows, next_field, source_span);
    }

    let scrutinee = ctx
        .resolve_bound_term_name(column)
        .unwrap_or_else(|| solveig_base::ice!("match column `{column}` was not bound"));
    let heads = match_heads_in_order(&rows);
    let mut case_arms = Vec::new();

    for head in heads {
        let field_count = match_head_field_count(&head, &rows);
        let field_names = fresh_match_field_names(next_field, field_count);
        let pattern = lower_match_head_pattern(ctx, &head, field_count);

        for field_name in &field_names {
            ctx.push_term(field_name);
        }

        let mut next_columns = field_names.clone();
        next_columns.extend_from_slice(rest_columns);
        let branch_rows = rows
            .iter()
            .cloned()
            .filter_map(|row| lower_match_head_row(row, &head, field_count, column))
            .collect::<Vec<_>>();
        let body = lower_match_columns(ctx, &next_columns, branch_rows, next_field, source_span);

        for _ in &field_names {
            ctx.pop();
        }

        case_arms.push(LowerCaseArm { pattern, body });
    }

    let default_rows = rows
        .into_iter()
        .filter_map(|row| lower_match_irrefutable_row(row, column))
        .collect::<Vec<_>>();

    if !default_rows.is_empty() {
        let body = lower_match_columns(ctx, rest_columns, default_rows, next_field, source_span);
        case_arms.push(LowerCaseArm {
            pattern: LowerPattern::Wild,
            body,
        });
    }

    Rc::new(IRExpr::Case {
        scrutinee,
        arms: case_arms,
    })
}

fn lower_match_row_body<A: Clone, P: Clone, S>(
    ctx: &mut LowerCtx,
    row: &LowerMatchRow<A, P>,
    source_span: &S,
) -> Rc<IRExpr>
where
    S: Fn(&A) -> Option<FileSpan>,
{
    let mut lowered_binders = Vec::with_capacity(row.bindings.len());

    for binding in &row.bindings {
        lowered_binders.push(
            ctx.resolve_bound_term_name(&binding.source)
                .unwrap_or_else(|| {
                    solveig_base::ice!("match binding source `{}` was not bound", binding.source)
                }),
        );
        ctx.push_term(&binding.name);
    }

    let mut body = lower_expr(ctx, &row.body, source_span);

    for _ in &row.bindings {
        ctx.pop();
    }

    for binder in lowered_binders.into_iter().rev() {
        body = Rc::new(IRExpr::Let(binder, body));
    }

    body
}

fn match_heads_in_order<A, P>(rows: &[LowerMatchRow<A, P>]) -> Vec<LowerMatchHead> {
    let mut heads = Vec::new();

    for row in rows {
        let Some(pattern) = row.patterns.first() else {
            continue;
        };
        let Some(head) = match_head(pattern) else {
            continue;
        };

        if !heads.iter().any(|seen| match_heads_equal(seen, &head)) {
            heads.push(head);
        }
    }

    heads
}

fn match_head<P>(pattern: &PatternNode<P>) -> Option<LowerMatchHead> {
    match &pattern.node {
        Pattern::Con(id, _) => Some(LowerMatchHead::Constructor(*id)),
        Pattern::Lit(lit) => Some(LowerMatchHead::Literal(lit.clone())),
        Pattern::Var(_) | Pattern::Wild => None,
    }
}

fn match_head_field_count<A, P>(head: &LowerMatchHead, rows: &[LowerMatchRow<A, P>]) -> usize {
    rows.iter()
        .filter_map(|row| row.patterns.first())
        .find_map(|pattern| match &pattern.node {
            Pattern::Con(id, args)
                if matches!(head, LowerMatchHead::Constructor(head_id) if id == head_id) =>
            {
                Some(args.len())
            }
            Pattern::Lit(lit) if matches!(head, LowerMatchHead::Literal(head_lit) if literals_equal(lit, head_lit)) => {
                Some(0)
            }
            _ => None,
        })
        .unwrap_or_else(|| solveig_base::ice!("match head had no source pattern"))
}

fn lower_match_head_pattern(
    ctx: &LowerCtx,
    head: &LowerMatchHead,
    field_count: usize,
) -> LowerPattern {
    match head {
        LowerMatchHead::Constructor(id) => {
            let info = ctx.constructors.get(id).unwrap_or_else(|| {
                solveig_base::ice!(
                    "unknown constructor in match pattern during lowering: {}",
                    ctx.constructor_name_for_error(*id)
                )
            });
            LowerPattern::Con(info.tag, vec![LowerPattern::Var; field_count])
        }
        LowerMatchHead::Literal(lit) => LowerPattern::Lit(lit.clone()),
    }
}

fn lower_match_default_row<A, P: Clone>(
    mut row: LowerMatchRow<A, P>,
    column: &str,
) -> LowerMatchRow<A, P> {
    let first = row
        .patterns
        .first()
        .cloned()
        .unwrap_or_else(|| solveig_base::ice!("match row missing pattern"));
    row.patterns.remove(0);
    add_irrefutable_binding(&mut row, first, column);
    row
}

fn lower_match_irrefutable_row<A, P: Clone>(
    row: LowerMatchRow<A, P>,
    column: &str,
) -> Option<LowerMatchRow<A, P>> {
    row.patterns
        .first()
        .is_some_and(|pattern| matches!(pattern.node, Pattern::Var(_) | Pattern::Wild))
        .then(|| lower_match_default_row(row, column))
}

fn lower_match_head_row<A, P: Clone>(
    mut row: LowerMatchRow<A, P>,
    head: &LowerMatchHead,
    field_count: usize,
    column: &str,
) -> Option<LowerMatchRow<A, P>> {
    let first = row.patterns.first().cloned()?;
    row.patterns.remove(0);

    match first.node {
        Pattern::Con(id, args) if matches!(head, LowerMatchHead::Constructor(head_id) if id == *head_id) =>
        {
            let mut patterns = args;
            patterns.extend(row.patterns);
            row.patterns = patterns;
            Some(row)
        }
        Pattern::Lit(lit) if matches!(head, LowerMatchHead::Literal(head_lit) if literals_equal(&lit, head_lit)) => {
            Some(row)
        }
        Pattern::Var(_) | Pattern::Wild => {
            let first_anno = first.anno;
            add_irrefutable_binding(
                &mut row,
                PatternNode::new(first.node, first_anno.clone()),
                column,
            );
            let mut patterns = vec![PatternNode::new(Pattern::Wild, first_anno); field_count];
            patterns.extend(row.patterns);
            row.patterns = patterns;
            Some(row)
        }
        _ => None,
    }
}

fn add_irrefutable_binding<A, P>(
    row: &mut LowerMatchRow<A, P>,
    pattern: PatternNode<P>,
    column: &str,
) {
    if let Pattern::Var(name) = pattern.node {
        row.bindings.push(LowerMatchBinding {
            name,
            source: column.to_string(),
        });
    }
}

fn fresh_match_field_names(next_field: &mut usize, count: usize) -> Vec<String> {
    (0..count)
        .map(|_| {
            let name = format!("$matchField{}", *next_field);
            *next_field = next_field
                .checked_add(1)
                .expect("ICE: match field name counter overflowed");
            name
        })
        .collect()
}

fn match_heads_equal(left: &LowerMatchHead, right: &LowerMatchHead) -> bool {
    match (left, right) {
        (LowerMatchHead::Constructor(left), LowerMatchHead::Constructor(right)) => left == right,
        (LowerMatchHead::Literal(left), LowerMatchHead::Literal(right)) => {
            literals_equal(left, right)
        }
        _ => false,
    }
}

fn literals_equal(left: &Literal, right: &Literal) -> bool {
    match (left, right) {
        (Literal::Int(left), Literal::Int(right)) => left == right,
        (Literal::Float(left), Literal::Float(right)) => left.to_bits() == right.to_bits(),
        (Literal::Char(left), Literal::Char(right)) => left == right,
        (Literal::Str(left), Literal::Str(right)) => left == right,
        _ => false,
    }
}

fn with_source(source: Option<FileSpan>, expr: Rc<IRExpr>) -> Rc<IRExpr> {
    source.map_or(expr.clone(), |source| {
        Rc::new(IRExpr::Located(source, expr))
    })
}

fn lower_builtin_global(def: &BuiltinDefinition) -> IRGlobalDef {
    let args = (0..def.arity)
        .map(|index| Rc::new(IRExpr::BVar(def.arity - index - 1)))
        .collect::<Vec<_>>();
    let mut body = Rc::new(IRExpr::CallBuiltin(def.builtin, args));

    for _ in 0..def.arity {
        body = Rc::new(IRExpr::Abs(body));
    }

    IRGlobalDef {
        name: def.name.clone(),
        body,
    }
}

fn lower_ev_app<A: Clone, P: Clone, S>(
    ctx: &mut LowerCtx,
    inner: &TermExprNode<A, P>,
    ev: &Ev,
    source_span: &S,
) -> Rc<IRExpr>
where
    S: Fn(&A) -> Option<FileSpan>,
{
    if let Some(method_id) = global_method_ref(inner)
        && let Some((class_id, field_index)) = ctx.dicts.method_field(method_id)
    {
        let dict = lower_ev(ctx, ev);
        return project_dict_field(ctx, class_id, dict, field_index);
    }

    let lowered_func = lower_expr(ctx, inner, source_span);
    let lowered_ev = lower_ev(ctx, ev);
    Rc::new(IRExpr::App(lowered_func, lowered_ev))
}

fn lower_ev(ctx: &mut LowerCtx, ev: &Ev) -> Rc<IRExpr> {
    match ev {
        Ev::Var(evvar) => ctx.resolve_ev_var(*evvar),
        Ev::Placeholder(evvar) => {
            solveig_base::ice!("evidence placeholder {evvar:?} survived into lowering")
        }
        Ev::Inst(instance_id, _ty_args, evidence_args) => {
            let global_name = ctx
                .dicts
                .instance_global(*instance_id)
                .unwrap_or_else(|| {
                    solveig_base::ice!("missing dictionary global for instance {instance_id:?}")
                })
                .to_string();
            let global_index = *ctx.globals.get(&global_name).unwrap_or_else(|| {
                solveig_base::ice!(
                    "dictionary global `{global_name}` was not registered before lowering"
                )
            });

            evidence_args
                .iter()
                .fold(Rc::new(IRExpr::FVar(global_index)), |acc, arg| {
                    Rc::new(IRExpr::App(acc, lower_ev(ctx, arg)))
                })
        }
        Ev::Super(root, class_id, index) => {
            let root = lower_ev(ctx, root);
            project_dict_field(ctx, *class_id, root, *index)
        }
    }
}

fn global_method_ref<A, P>(expr: &TermExprNode<A, P>) -> Option<DefId> {
    match &expr.node {
        TermExpr::Var(VarRef::Global(id)) => Some(*id),
        TermExpr::TyApp(inner, _) | TermExpr::Anno(inner, _) => global_method_ref(inner),
        _ => None,
    }
}

fn project_dict_field(
    ctx: &LowerCtx,
    class_id: ClassId,
    dict: Rc<IRExpr>,
    field_index: usize,
) -> Rc<IRExpr> {
    let layout = ctx
        .dicts
        .class(class_id)
        .unwrap_or_else(|| solveig_base::ice!("missing dictionary layout for class {class_id:?}"));

    project_dict_layout_field(layout, dict, field_index)
}

fn project_dict_layout_field(
    layout: &DictLayout,
    dict: Rc<IRExpr>,
    field_index: usize,
) -> Rc<IRExpr> {
    let arity = usize::from(layout.arity);
    assert!(
        field_index < arity,
        "ICE: dictionary field {} out of bounds for {} with arity {}",
        field_index,
        layout.name,
        arity
    );

    let field_patterns = (0..arity)
        .map(|index| {
            if index == field_index {
                LowerPattern::Var
            } else {
                LowerPattern::Wild
            }
        })
        .collect();

    Rc::new(IRExpr::Case {
        scrutinee: dict,
        arms: vec![LowerCaseArm {
            pattern: LowerPattern::Con(layout.tag, field_patterns),
            body: Rc::new(IRExpr::BVar(0)),
        }],
    })
}

fn build_dict_value(layout: &DictLayout, fields: Vec<Rc<IRExpr>>) -> Rc<IRExpr> {
    assert_eq!(
        fields.len(),
        usize::from(layout.arity),
        "ICE: dictionary {} expected {} fields, got {}",
        layout.name,
        layout.arity,
        fields.len()
    );

    fields.into_iter().fold(
        Rc::new(IRExpr::Con {
            tag: layout.tag,
            arity: layout.arity,
        }),
        |acc, field| Rc::new(IRExpr::App(acc, field)),
    )
}

fn lower_case_arm<A: Clone, P: Clone, S>(
    ctx: &mut LowerCtx,
    arm: &CaseArm<A, P>,
    source_span: &S,
) -> LowerCaseArm<IRExpr>
where
    S: Fn(&A) -> Option<FileSpan>,
{
    let ir_pattern = lower_pattern(ctx, &arm.pattern);
    ctx.push_pattern_binders(&arm.pattern);
    let lowered_body = lower_expr(ctx, &arm.expr, source_span);
    ctx.pop_pattern_binders(&arm.pattern);
    LowerCaseArm {
        pattern: ir_pattern,
        body: lowered_body,
    }
}

fn lower_pattern<A>(ctx: &LowerCtx, pattern: &PatternNode<A>) -> LowerPattern {
    match &pattern.node {
        Pattern::Var(_) => LowerPattern::Var,
        Pattern::Wild => LowerPattern::Wild,
        Pattern::Lit(lit_val) => LowerPattern::Lit(lit_val.clone()),
        Pattern::Con(id, sub_patterns) => {
            let info = ctx.constructors.get(id).unwrap_or_else(|| {
                solveig_base::ice!(
                    "unknown constructor in pattern during lowering: {}",
                    ctx.constructor_name_for_error(*id)
                )
            });
            let sub_ir_patterns = sub_patterns
                .iter()
                .map(|sub_pattern| lower_pattern(ctx, sub_pattern))
                .collect();
            LowerPattern::Con(info.tag, sub_ir_patterns)
        }
    }
}

fn lower_instance_dict_global<A: Clone, P: Clone, S>(
    ctx: &mut LowerCtx,
    inst: &InstanceDefinition<A, P>,
    source_span: &S,
) -> IRGlobalDef
where
    S: Fn(&A) -> Option<FileSpan>,
{
    let layout = ctx
        .dicts
        .class(inst.head.class_id)
        .unwrap_or_else(|| {
            solveig_base::ice!(
                "missing dictionary layout for instance head {:?}",
                inst.head.class_id
            )
        })
        .clone();

    assert_eq!(
        inst.context.len(),
        inst.context_evidence.len(),
        "ICE: instance {:?} context/evidence arity mismatch",
        inst.id
    );
    assert_eq!(
        inst.superclass_evidence.len(),
        layout.superclass_count,
        "ICE: instance {:?} superclass/evidence arity mismatch",
        inst.id
    );

    for evvar in &inst.context_evidence {
        ctx.push_evidence(*evvar);
    }

    let mut fields = Vec::new();

    for ev in &inst.superclass_evidence {
        fields.push(lower_ev(ctx, ev));
    }

    for method_layout in &layout.methods {
        let impl_method = inst
            .methods
            .iter()
            .find(|method| {
                method.name == method_layout.name || method.name == method_layout.short_name
            })
            .unwrap_or_else(|| {
                solveig_base::ice!(
                    "instance {:?} missing implementation for class method {}",
                    inst.id,
                    method_layout.name
                )
            });

        let mut field = lower_expr(ctx, &impl_method.body, source_span);
        for evvar in &inst.context_evidence {
            field = Rc::new(IRExpr::App(field, ctx.resolve_ev_var(*evvar)));
        }
        fields.push(field);
    }

    let mut body = build_dict_value(&layout, fields);

    for _ in &inst.context_evidence {
        ctx.pop();
    }

    for _ in inst.context_evidence.iter().rev() {
        body = Rc::new(IRExpr::Abs(body));
    }

    IRGlobalDef {
        name: instance_global_name(inst.id),
        body,
    }
}

fn build_constructor_table<A, P>(
    module: &ResolvedModule<A, P>,
    names: &Names,
) -> FxHashMap<CtorId, LowerConstructorInfo> {
    let mut table = FxHashMap::default();
    for data_def in &module.data_defs {
        for constructor_def in &data_def.constructors {
            let table_info = module
                .ctor_table
                .get(constructor_def.id)
                .unwrap_or_else(|| {
                    solveig_base::ice!(
                        "lowering constructor table missing CtorId {:?}",
                        constructor_def.id
                    )
                });
            let arity = u16::try_from(constructor_def.fields.len())
                .expect("ICE: constructor arity exceeded u16 capacity");
            table.insert(
                constructor_def.id,
                LowerConstructorInfo {
                    name: names.symbol_display_name(table_info.name),
                    short_name: names.symbol_display_name(table_info.short_name),
                    tag: table_info.tag,
                    arity,
                },
            );
        }
    }
    table
}

fn next_synthetic_constructor_base(
    constructors: &FxHashMap<CtorId, LowerConstructorInfo>,
) -> (u32, u16) {
    let next_ctor = constructors
        .keys()
        .map(|ctor| ctor.0)
        .max()
        .map_or(0, |id| {
            id.checked_add(1)
                .expect("ICE: constructor id space exhausted before dictionary lowering")
        });
    let next_tag = constructors
        .values()
        .map(|info| info.tag)
        .max()
        .map_or(0, |tag| {
            tag.checked_add(1)
                .expect("ICE: constructor tag space exhausted before dictionary lowering")
        });

    (next_ctor, next_tag)
}

pub fn lower_module<A: Clone, P: Clone, S>(
    module: &ResolvedModule<A, P>,
    names: &Names,
    source_span: S,
) -> IRProgram
where
    S: Fn(ModuleId, &A) -> Option<FileSpan>,
{
    let constructors = build_constructor_table(module, names);
    let (next_ctor_id, next_tag) = next_synthetic_constructor_base(&constructors);
    let dicts = DictLayouts::build(module, next_ctor_id, next_tag);

    let mut globals: FxHashMap<String, usize> = FxHashMap::default();
    for (index, func_def) in module.func_defs.iter().enumerate() {
        globals.insert(func_def.name.clone(), index);
    }
    for (offset, builtin_def) in module.builtin_defs.iter().enumerate() {
        globals.insert(builtin_def.name.clone(), module.func_defs.len() + offset);
    }
    let instance_offset = module.func_defs.len() + module.builtin_defs.len();
    for (offset, inst) in module.instance_defs.iter().enumerate() {
        globals.insert(instance_global_name(inst.id), instance_offset + offset);
    }

    let mut ctx = LowerCtx {
        names: names.clone(),
        scope: Vec::new(),
        globals,
        constructors: constructors.clone(),
        dicts: dicts.clone(),
        string_constants: Vec::new(),
        string_indices: FxHashMap::default(),
    };

    let mut ir_globals = module
        .func_defs
        .iter()
        .map(|func_def| {
            let source_span = |anno: &A| source_span(func_def.id.module, anno);
            let lowered_body = lower_expr(&mut ctx, &func_def.body, &source_span);
            IRGlobalDef {
                name: func_def.name.clone(),
                body: lowered_body,
            }
        })
        .collect::<Vec<_>>();

    ir_globals.extend(module.builtin_defs.iter().map(lower_builtin_global));

    ir_globals.extend(module.instance_defs.iter().map(|inst| {
        let source_span = |anno: &A| source_span(inst.def_module, anno);
        lower_instance_dict_global(&mut ctx, inst, &source_span)
    }));

    let mut constructor_arities = constructors
        .iter()
        .map(|(ctor, info)| CtorRuntimeInfo {
            ctor: *ctor,
            role: runtime_ctor_role(names, *ctor),
            name: info.name.clone(),
            short_name: info.short_name.clone(),
            tag: info.tag,
            arity: info.arity,
        })
        .collect::<Vec<_>>();
    constructor_arities.extend(dicts.runtime_constructors().iter().cloned());

    IRProgram {
        globals: ir_globals,
        constructor_arities,
        string_constants: ctx.string_constants,
        string_literals: names.string_literals().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use solveig_ast::ast::{
        ClassDefinition, ClassMethodDef, ClassTable, InstanceId, InstanceMethodDef, ModuleId,
        Names, Predicate, Scheme, TermDefinition, Ty, TyConTable, VarRef,
    };
    use solveig_base::{FileId, Span};

    use super::*;

    fn node(expr: TermExpr<(), ()>) -> TermExprNode<(), ()> {
        TermExprNode::new(expr, ())
    }

    #[test]
    fn class_method_evidence_application_lowers_to_dictionary_projection() {
        let mut names = Names::new();
        let class_id = ClassId(0);
        let method_id = names.def_id("test.Eq.eq");
        let instance_id = InstanceId(0);
        let pred = Predicate {
            class_id,
            args: Vec::new(),
        };

        let module = ResolvedModule {
            data_defs: Vec::new(),
            class_defs: vec![ClassDefinition {
                id: class_id,
                name: "test.Eq".to_string(),
                short_name: "Eq".to_string(),
                params: Vec::new(),
                superclasses: Vec::new(),
                methods: vec![ClassMethodDef {
                    id: method_id,
                    name: "test.Eq.eq".to_string(),
                    short_name: "eq".to_string(),
                    scheme: Scheme {
                        quantifiers: Vec::new(),
                        predicates: vec![pred.clone()],
                        ty: Ty::TyUnit,
                    },
                    default_body: None,
                }],
            }],
            instance_defs: vec![InstanceDefinition {
                id: instance_id,
                span: 0..0,
                def_module: ModuleId(0),
                class_module: Some(ModuleId(0)),
                type_module: Some(ModuleId(0)),
                context: Vec::new(),
                context_evidence: Vec::new(),
                head: pred,
                superclass_evidence: Vec::new(),
                methods: vec![InstanceMethodDef {
                    name: "eq".to_string(),
                    body: node(TermExpr::Unit),
                }],
            }],
            builtin_defs: Vec::new(),
            tycon_table: TyConTable::new(),
            ctor_table: Default::default(),
            class_table: ClassTable::new(),
            tycon_kinds: FxHashMap::default(),
            constructor_types: FxHashMap::default(),
            func_schemes: FxHashMap::default(),
            func_defs: vec![TermDefinition {
                id: names.def_id("main.main"),
                name: "main.main".to_string(),
                short_name: "main".to_string(),
                signature: None,
                body: node(TermExpr::EvApp(
                    Box::new(node(TermExpr::Var(VarRef::Global(method_id)))),
                    Ev::Inst(instance_id, Vec::new(), Vec::new()),
                )),
            }],
        };

        let expected_source = FileSpan::new(FileId(3), Span::new(20, 24));
        let ir = lower_module(&module, &names, |_, _| Some(expected_source));

        assert_eq!(ir.globals.len(), 2);
        let dict_tag = ir
            .constructor_arities
            .iter()
            .find(|info| info.name == "$dict.test.Eq.Mk")
            .expect("dictionary constructor should be registered")
            .tag;

        let IRExpr::Located(source, body) = ir.globals[0].body.as_ref() else {
            solveig_base::ice!("expected source-located lowered expression")
        };
        assert_eq!(*source, expected_source);

        match body.as_ref() {
            IRExpr::Case { scrutinee, arms } => {
                assert!(matches!(scrutinee.as_ref(), IRExpr::FVar(1)));
                assert_eq!(arms.len(), 1);
                assert!(matches!(
                    &arms[0].pattern,
                    LowerPattern::Con(tag, fields)
                        if *tag == dict_tag && matches!(fields.as_slice(), [LowerPattern::Var])
                ));
                assert!(matches!(arms[0].body.as_ref(), IRExpr::BVar(0)));
            }
            other => solveig_base::ice!("expected dictionary projection case, got {other:?}"),
        }
    }
}
