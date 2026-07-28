//! Lowering from checked terms into VM-oriented core syntax.
//! This pass assigns globals, lowers constructors, and prepares IO primitives.

use solveig_base::FxHashMap;
use std::rc::Rc;

use solveig_ast::ast::{
    BuiltinDefinition, CaseArm, ClassId, CtorId, CtorRuntimeInfo, DefId, Ev, EvVarId,
    InstanceDefinition, Literal, Names, Pattern, PatternNode, ResolvedModule, StrLitId, TermExpr,
    TermExprNode, VarRef, runtime_ctor_role,
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

impl LowerCtx {
    fn resolve_var(&self, var: &VarRef) -> Rc<IRExpr> {
        let name = self.names.var_display_name(var);

        for (depth, binding) in self.scope.iter().rev().enumerate() {
            if matches!(binding, Binding::Term(bound_name) if bound_name == &name) {
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
        panic!("ICE: unbound variable during lowering: {name}");
    }

    fn resolve_ev_var(&self, evvar: EvVarId) -> Rc<IRExpr> {
        for (depth, binding) in self.scope.iter().rev().enumerate() {
            if matches!(binding, Binding::Evidence(bound_evvar) if *bound_evvar == evvar) {
                return Rc::new(IRExpr::BVar(depth));
            }
        }

        panic!("ICE: unbound evidence variable during lowering: {evvar:?}");
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

fn lower_expr<A, P>(ctx: &mut LowerCtx, expr: &TermExprNode<A, P>) -> Rc<IRExpr> {
    match &expr.node {
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
                panic!(
                    "ICE: unknown constructor during lowering: {}",
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
        TermExpr::TyApp(inner, _) | TermExpr::Anno(inner, _) => lower_expr(ctx, inner),
        TermExpr::EvAbs(evvar, _, body) => {
            ctx.push_evidence(*evvar);
            let lowered_body = lower_expr(ctx, body);
            ctx.pop();
            Rc::new(IRExpr::Abs(lowered_body))
        }
        TermExpr::EvApp(inner, ev) => lower_ev_app(ctx, inner, ev),
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
        TermExpr::Let(name, binder, in_expr) => {
            let lowered_binder = lower_expr(ctx, binder);

            ctx.push_term(name);
            let lowered_in_expr = lower_expr(ctx, in_expr);
            ctx.pop();

            Rc::new(IRExpr::Let(lowered_binder, lowered_in_expr))
        }
        TermExpr::Hole => panic!("ICE: driver allowed a typed hole to reach lowering"),
    }
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

fn lower_ev_app<A, P>(ctx: &mut LowerCtx, inner: &TermExprNode<A, P>, ev: &Ev) -> Rc<IRExpr> {
    if let Some(method_id) = global_method_ref(inner)
        && let Some((class_id, field_index)) = ctx.dicts.method_field(method_id)
    {
        let dict = lower_ev(ctx, ev);
        return project_dict_field(ctx, class_id, dict, field_index);
    }

    let lowered_func = lower_expr(ctx, inner);
    let lowered_ev = lower_ev(ctx, ev);
    Rc::new(IRExpr::App(lowered_func, lowered_ev))
}

fn lower_ev(ctx: &mut LowerCtx, ev: &Ev) -> Rc<IRExpr> {
    match ev {
        Ev::Var(evvar) => ctx.resolve_ev_var(*evvar),
        Ev::Placeholder(evvar) => {
            panic!("ICE: evidence placeholder {evvar:?} survived into lowering")
        }
        Ev::Inst(instance_id, _ty_args, evidence_args) => {
            let global_name = ctx
                .dicts
                .instance_global(*instance_id)
                .unwrap_or_else(|| {
                    panic!("ICE: missing dictionary global for instance {instance_id:?}")
                })
                .to_string();
            let global_index = *ctx.globals.get(&global_name).unwrap_or_else(|| {
                panic!("ICE: dictionary global `{global_name}` was not registered before lowering")
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
        .unwrap_or_else(|| panic!("ICE: missing dictionary layout for class {class_id:?}"));

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

fn lower_case_arm<A, P>(ctx: &mut LowerCtx, arm: &CaseArm<A, P>) -> LowerCaseArm<IRExpr> {
    let ir_pattern = lower_pattern(ctx, &arm.pattern);
    ctx.push_pattern_binders(&arm.pattern);
    let lowered_body = lower_expr(ctx, &arm.expr);
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
                panic!(
                    "ICE: unknown constructor in pattern during lowering: {}",
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

fn lower_instance_dict_global<A, P>(
    ctx: &mut LowerCtx,
    inst: &InstanceDefinition<A, P>,
) -> IRGlobalDef {
    let layout = ctx
        .dicts
        .class(inst.head.class_id)
        .unwrap_or_else(|| {
            panic!(
                "ICE: missing dictionary layout for instance head {:?}",
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
                panic!(
                    "ICE: instance {:?} missing implementation for class method {}",
                    inst.id, method_layout.name
                )
            });

        let mut field = lower_expr(ctx, &impl_method.body);
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
                    panic!(
                        "ICE: lowering constructor table missing CtorId {:?}",
                        constructor_def.id
                    )
                });
            let arity = constructor_def.fields.len() as u16;
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

pub fn lower_module<A, P>(module: &ResolvedModule<A, P>, names: &Names) -> IRProgram {
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
            let lowered_body = lower_expr(&mut ctx, &func_def.body);
            IRGlobalDef {
                name: func_def.name.clone(),
                body: lowered_body,
            }
        })
        .collect::<Vec<_>>();

    ir_globals.extend(module.builtin_defs.iter().map(lower_builtin_global));

    ir_globals.extend(
        module
            .instance_defs
            .iter()
            .map(|inst| lower_instance_dict_global(&mut ctx, inst)),
    );

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

        let ir = lower_module(&module, &names);

        assert_eq!(ir.globals.len(), 2);
        let dict_tag = ir
            .constructor_arities
            .iter()
            .find(|info| info.name == "$dict.test.Eq.Mk")
            .expect("dictionary constructor should be registered")
            .tag;

        match ir.globals[0].body.as_ref() {
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
            other => panic!("ICE: expected dictionary projection case, got {other:?}"),
        }
    }
}
