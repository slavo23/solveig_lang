//! Validation for checked core terms after type checking and evidence elaboration.
//! The driver runs this before lowering to catch internal consistency errors.

use solveig_base::FxHashMap;
use std::collections::BTreeSet;
use std::ops::Range;

use solveig_ast::ast::{
    CaseArmNode, ClassDefinition, ClassId, ClassTable, CtorId, CtorTable, DefId, Ev, EvVarId,
    InstanceDefinition, InstanceId, Kind, ModuleId, Names, Pattern, PatternNode, Predicate, Scheme,
    TermExpr, TermExprNode, Ty, TyConTable, TyVarId, VarRef,
};
use solveig_base::{Diagnostic, Label};
use solveig_base::{FileId, FileSpan, Span as SourceSpan};

use crate::inference::type_infer::{CheckedModule, TcMetadata, TypeCheckEnv};

#[derive(Clone, Debug)]
struct CoreInstance {
    head: Predicate,
    context: Vec<Predicate>,
}

#[derive(Clone, Debug, Default)]
pub struct CoreLintEnv {
    file_ids: FxHashMap<ModuleId, FileId>,
    func_schemes: FxHashMap<DefId, Scheme>,
    class_defs: FxHashMap<ClassId, ClassDefinition>,
    instances: FxHashMap<InstanceId, CoreInstance>,
    ctor_table: CtorTable,
    tycon_table: TyConTable,
    class_table: ClassTable,
    names: Names,
}

impl CoreLintEnv {
    pub fn from_module(module: &CheckedModule, names: &Names) -> Self {
        let mut env = Self {
            names: names.clone(),
            ..Self::default()
        };
        env.extend_from_checked_module(module);
        env
    }

    pub fn from_typecheck_env(module: &CheckedModule, check_env: &TypeCheckEnv) -> Self {
        let mut env = Self {
            func_schemes: check_env.func_schemes.clone(),
            class_defs: check_env.class_defs.clone(),
            ctor_table: check_env.ctor_table.clone(),
            tycon_table: check_env.tycon_table.clone(),
            class_table: check_env.class_table.clone(),
            names: check_env.names.clone(),
            ..Self::default()
        };

        for inst in &check_env.instance_defs {
            env.insert_instance(inst.id, inst.head.clone(), inst.context.clone());
        }

        env.extend_from_checked_module(module);
        env
    }

    pub fn with_file(mut self, module: ModuleId, file: FileId) -> Self {
        self.file_ids.insert(module, file);
        self
    }

    pub fn with_files(mut self, files: FxHashMap<ModuleId, FileId>) -> Self {
        self.file_ids.extend(files);
        self
    }

    fn extend_from_checked_module(&mut self, module: &CheckedModule) {
        self.func_schemes.extend(module.func_schemes.clone());

        for class_def in &module.class_defs {
            self.class_defs.insert(class_def.id, class_def.clone());
        }

        for inst in &module.instance_defs {
            self.insert_instance(inst.id, inst.head.clone(), inst.context.clone());
        }

        for (id, info) in module.ctor_table.iter() {
            self.ctor_table.insert_with_id(id, info.clone());
        }
        for (id, info) in module.tycon_table.iter() {
            self.tycon_table.insert_with_id(id, info.clone());
        }
        for (id, info) in module.class_table.iter() {
            self.class_table.insert_with_id(id, info.clone());
        }
    }

    fn insert_instance(&mut self, id: InstanceId, head: Predicate, context: Vec<Predicate>) {
        self.instances.insert(id, CoreInstance { head, context });
    }

    fn class_method_scheme(&self, id: DefId) -> Option<&Scheme> {
        self.class_defs
            .values()
            .flat_map(|class_def| class_def.methods.iter())
            .find(|method| method.id == id)
            .map(|method| &method.scheme)
    }

    fn file_span(&self, module: ModuleId, span: &Range<usize>) -> Option<FileSpan> {
        self.file_ids
            .get(&module)
            .copied()
            .map(|file| FileSpan::new(file, SourceSpan::from_usize_range(span.clone())))
    }
}

pub fn lint_checked_module(module: &CheckedModule, env: &CoreLintEnv) -> Vec<Diagnostic> {
    let mut linter = CoreLinter {
        env,
        diagnostics: Vec::new(),
        evidence_env: FxHashMap::default(),
        current_owner: None,
    };

    linter.lint_module(module);
    linter.diagnostics
}

struct CoreLinter<'a> {
    env: &'a CoreLintEnv,
    diagnostics: Vec<Diagnostic>,
    evidence_env: FxHashMap<EvVarId, Predicate>,
    current_owner: Option<Owner>,
}

#[derive(Clone, Debug)]
struct Owner {
    name: String,
    module: ModuleId,
}

impl CoreLinter<'_> {
    fn lint_module(&mut self, module: &CheckedModule) {
        for scheme in module.func_schemes.values() {
            self.lint_scheme(scheme);
        }

        for constructor_type in module.constructor_types.values() {
            self.lint_type(constructor_type);
        }

        for class_def in &module.class_defs {
            for superclass in &class_def.superclasses {
                self.lint_predicate(superclass);
                self.check_class_exists(superclass.class_id, "class superclass predicate");
            }
            for method in &class_def.methods {
                self.lint_scheme(&method.scheme);
            }
        }

        for func_def in &module.func_defs {
            self.with_owner(func_def.name.clone(), func_def.id.module, |this| {
                if let Some(signature) = &func_def.signature {
                    this.lint_scheme(signature);
                    this.check_definition_predicate_binders(signature, &func_def.body);
                }
                this.lint_expr(&func_def.body);
            });
        }

        for inst in &module.instance_defs {
            self.lint_instance(inst);
        }
    }

    fn lint_instance(&mut self, inst: &InstanceDefinition<TcMetadata, Range<usize>>) {
        let owner = format!(
            "instance#{} {}",
            inst.id.0,
            self.pretty_predicate_fallback(&inst.head)
        );
        self.with_owner(owner, inst.def_module, |this| {
            this.lint_predicate(&inst.head);
            this.check_class_exists(inst.head.class_id, "instance head");
            for pred in &inst.context {
                this.lint_predicate(pred);
                this.check_class_exists(pred.class_id, "instance context");
            }

            if inst.context.len() != inst.context_evidence.len() {
                this.report(
                    None,
                    format!(
                        "ICE: instance context/evidence arity mismatch: {} predicates, {} evidence variables",
                        inst.context.len(),
                        inst.context_evidence.len()
                    ),
                );
            }

            for (evvar, pred) in inst.context_evidence.iter().zip(inst.context.iter()) {
                this.evidence_env.insert(*evvar, pred.clone());
            }

            let expected_superclasses = this
                .env
                .class_defs
                .get(&inst.head.class_id)
                .map(|class_def| {
                    let subst = subst_from_class_head(&class_def.params, &inst.head.args);
                    class_def
                        .superclasses
                        .iter()
                        .map(|pred| substitute_predicate(pred, &subst))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            if expected_superclasses.len() != inst.superclass_evidence.len() {
                this.report(
                    None,
                    format!(
                        "ICE: instance superclass/evidence arity mismatch: {} superclasses, {} evidence values",
                        expected_superclasses.len(),
                        inst.superclass_evidence.len()
                    ),
                );
            }

            for (index, ev) in inst.superclass_evidence.iter().enumerate() {
                let Some(actual) = this.infer_ev_predicate(ev) else {
                    continue;
                };
                if let Some(expected) = expected_superclasses.get(index) {
                    this.check_predicate_matches(
                        expected,
                        &actual,
                        None,
                        &format!("instance superclass evidence {index}"),
                    );
                }
            }

            for method in &inst.methods {
                this.lint_expr(&method.body);
            }

            for evvar in &inst.context_evidence {
                this.evidence_env.remove(evvar);
            }
        });
    }

    fn with_owner(&mut self, name: String, module: ModuleId, f: impl FnOnce(&mut Self)) {
        let previous = self.current_owner.replace(Owner { name, module });
        f(self);
        self.current_owner = previous;
    }

    fn lint_expr(&mut self, expr: &TermExprNode<TcMetadata, Range<usize>>) {
        if expr_requires_source_span(&expr.node) {
            self.check_span(&expr.anno.span);
        }
        self.lint_type(&expr.anno.inferred_type);

        match &expr.node {
            TermExpr::Unit | TermExpr::Lit(_) | TermExpr::Hole => {}
            TermExpr::Var(var) => {
                if let VarRef::Global(id) = var {
                    self.check_global_exists(*id, &expr.anno.span);
                }
            }
            TermExpr::Con(id) => self.check_ctor_exists(*id, &expr.anno.span),
            TermExpr::App(func, arg) => {
                self.lint_expr(func);
                self.lint_expr(arg);
            }
            TermExpr::Abs(_, annotation, body) => {
                if let Some(ty) = annotation {
                    self.lint_type(ty);
                }
                self.lint_expr(body);
            }
            TermExpr::TyAbs(_, _, body) => self.lint_expr(body),
            TermExpr::TyApp(body, ty) | TermExpr::Anno(body, ty) => {
                self.lint_type(ty);
                self.lint_expr(body);
            }
            TermExpr::EvAbs(evvar, pred, body) => {
                self.lint_predicate(pred);
                let old = self.evidence_env.insert(*evvar, pred.clone());
                self.lint_expr(body);
                match old {
                    Some(previous) => {
                        self.evidence_env.insert(*evvar, previous);
                    }
                    None => {
                        self.evidence_env.remove(evvar);
                    }
                }
            }
            TermExpr::EvApp(_, _) => self.lint_ev_app_chain(expr),
            TermExpr::Case(scrutinee, arms) => {
                self.lint_expr(scrutinee);
                for arm in arms {
                    self.lint_case_arm(arm);
                }
            }
            TermExpr::IfThenElse(cond, then_branch, else_branch) => {
                self.lint_expr(cond);
                self.lint_expr(then_branch);
                self.lint_expr(else_branch);
            }
            TermExpr::Let(_, binder, body) => {
                self.lint_expr(binder);
                self.lint_expr(body);
            }
        }
    }

    fn lint_case_arm(&mut self, arm: &CaseArmNode<TcMetadata, Range<usize>>) {
        self.lint_pattern(&arm.node.pattern);
        self.lint_expr(&arm.node.expr);
    }

    fn check_definition_predicate_binders(
        &mut self,
        scheme: &Scheme,
        body: &TermExprNode<TcMetadata, Range<usize>>,
    ) {
        let mut current = body;

        for _ in &scheme.quantifiers {
            let TermExpr::TyAbs(_, _, inner) = &current.node else {
                break;
            };
            current = inner;
        }

        let mut actual = Vec::new();
        while let TermExpr::EvAbs(_, pred, inner) = &current.node {
            actual.push(pred.clone());
            current = inner;
        }

        if actual.len() != scheme.predicates.len() {
            self.report(
                Some(body.anno.span.clone()),
                format!(
                    "ICE: scheme/evidence binder arity mismatch: {} predicates, {} evidence binders",
                    scheme.predicates.len(),
                    actual.len()
                ),
            );
            return;
        }

        for (index, (expected, actual)) in scheme.predicates.iter().zip(actual.iter()).enumerate() {
            if expected != actual {
                self.report(
                    Some(body.anno.span.clone()),
                    format!(
                        "ICE: scheme/evidence binder {index} predicate mismatch: expected `{}`, got `{}`",
                        self.pretty_predicate_fallback(expected),
                        self.pretty_predicate_fallback(actual)
                    ),
                );
            }
        }
    }

    fn lint_pattern(&mut self, pattern: &PatternNode<Range<usize>>) {
        self.check_span(&pattern.anno);

        match &pattern.node {
            Pattern::Con(id, sub_patterns) => {
                self.check_ctor_exists(*id, &pattern.anno);
                for sub_pattern in sub_patterns {
                    self.lint_pattern(sub_pattern);
                }
            }
            Pattern::Lit(_) | Pattern::Var(_) | Pattern::Wild => {}
        }
    }

    fn lint_ev_app_chain(&mut self, expr: &TermExprNode<TcMetadata, Range<usize>>) {
        let (head, evidence) = collect_ev_app_spine(expr);
        let expected = self.expected_predicates_for_head(head);

        if expected.len() < evidence.len() {
            self.report(
                Some(expr.anno.span.clone()),
                format!(
                    "ICE: evidence application supplies {} dictionaries but head expects {}",
                    evidence.len(),
                    expected.len()
                ),
            );
        }

        let mut subst = FxHashMap::default();
        for (index, ev) in evidence.iter().enumerate() {
            let Some(actual) = self.infer_ev_predicate(ev) else {
                continue;
            };

            let Some(expected_pred) = expected.get(index) else {
                continue;
            };

            self.check_predicate_matches(
                expected_pred,
                &actual,
                Some(expr.anno.span.clone()),
                &format!("evidence argument {index}"),
            );
            match_predicate_pattern(expected_pred, &actual, &mut subst);
        }

        self.lint_expr(head);
    }

    fn expected_predicates_for_head(
        &mut self,
        head: &TermExprNode<TcMetadata, Range<usize>>,
    ) -> Vec<Predicate> {
        let (_, preds, _) = head.anno.inferred_type.peel_scheme_like();
        if !preds.is_empty() {
            return preds;
        }

        let Some(global_id) = global_head_id(head) else {
            return Vec::new();
        };

        self.env
            .func_schemes
            .get(&global_id)
            .or_else(|| self.env.class_method_scheme(global_id))
            .map(|scheme| scheme.predicates.clone())
            .unwrap_or_default()
    }

    fn infer_ev_predicate(&mut self, ev: &Ev) -> Option<Predicate> {
        match ev {
            Ev::Var(evvar) => match self.evidence_env.get(evvar) {
                Some(pred) => Some(pred.clone()),
                None => {
                    self.report(
                        None,
                        format!("ICE: unbound evidence variable %ev{}", evvar.0),
                    );
                    None
                }
            },
            Ev::Placeholder(evvar) => {
                self.report(
                    None,
                    format!(
                        "ICE: evidence placeholder %ev{} survived into checked core",
                        evvar.0
                    ),
                );
                None
            }
            Ev::Inst(instance_id, ty_args, sub_evidence) => {
                let Some(inst) = self.env.instances.get(instance_id).cloned() else {
                    self.report(
                        None,
                        format!(
                            "ICE: evidence references unknown instance#{}",
                            instance_id.0
                        ),
                    );
                    return None;
                };

                if inst.context.len() != sub_evidence.len() {
                    self.report(
                        None,
                        format!(
                            "ICE: instance#{} evidence arity mismatch: {} context predicates, {} evidence arguments",
                            instance_id.0,
                            inst.context.len(),
                            sub_evidence.len()
                        ),
                    );
                }

                let vars = evidence_ty_vars_for_instance(&inst.head, &inst.context);
                if vars.len() != ty_args.len() {
                    self.report(
                        None,
                        format!(
                            "ICE: instance#{} type evidence arity mismatch: {} type variables, {} type arguments",
                            instance_id.0,
                            vars.len(),
                            ty_args.len()
                        ),
                    );
                }
                for ty in ty_args {
                    self.lint_type(ty);
                }

                let subst = vars
                    .into_iter()
                    .zip(ty_args.iter().cloned())
                    .collect::<FxHashMap<_, _>>();

                for (index, (expected_context, ev)) in
                    inst.context.iter().zip(sub_evidence.iter()).enumerate()
                {
                    let expected = substitute_predicate(expected_context, &subst);
                    if let Some(actual) = self.infer_ev_predicate(ev) {
                        self.check_predicate_matches(
                            &expected,
                            &actual,
                            None,
                            &format!("instance#{} context evidence {}", instance_id.0, index),
                        );
                    }
                }

                Some(substitute_predicate(&inst.head, &subst))
            }
            Ev::Super(root, class_id, index) => {
                let root_pred = self.infer_ev_predicate(root)?;

                if root_pred.class_id != *class_id {
                    self.report(
                        None,
                        format!(
                            "ICE: EvSuper source class mismatch: evidence has class#{}, projection says class#{}",
                            root_pred.class_id.0, class_id.0
                        ),
                    );
                }

                let Some(class_def) = self.env.class_defs.get(class_id).cloned() else {
                    self.report(
                        None,
                        format!("ICE: EvSuper references unknown class#{}", class_id.0),
                    );
                    return None;
                };

                if *index >= class_def.superclasses.len() {
                    self.report(
                        None,
                        format!(
                            "ICE: EvSuper index {} out of range for class `{}` with {} superclasses",
                            index,
                            class_def.name,
                            class_def.superclasses.len()
                        ),
                    );
                    return None;
                }

                let subst = subst_from_class_head(&class_def.params, &root_pred.args);
                Some(substitute_predicate(
                    &class_def.superclasses[*index],
                    &subst,
                ))
            }
        }
    }

    fn lint_scheme(&mut self, scheme: &Scheme) {
        for pred in &scheme.predicates {
            self.lint_predicate(pred);
            self.check_class_exists(pred.class_id, "scheme predicate");
        }
        self.lint_type(&scheme.ty);
    }

    fn lint_predicate(&mut self, pred: &Predicate) {
        for arg in &pred.args {
            self.lint_type(arg);
        }
    }

    fn lint_type(&mut self, ty: &Ty) {
        match ty {
            Ty::TyExVar(evar) => self.report(
                None,
                format!(
                    "ICE: unresolved existential ?{} remains in checked core",
                    evar.0
                ),
            ),
            Ty::TyArr(lhs, rhs) | Ty::TyApp(lhs, rhs) => {
                self.lint_type(lhs);
                self.lint_type(rhs);
            }
            Ty::TyForAll(_, preds, body) => {
                for pred in preds {
                    self.lint_predicate(pred);
                    self.check_class_exists(pred.class_id, "forall predicate");
                }
                self.lint_type(body);
            }
            Ty::TyUnit | Ty::TyVar(_) | Ty::TyCon(_) => {}
        }
    }

    fn check_span(&mut self, span: &Range<usize>) {
        if span.start >= span.end {
            self.report(
                Some(span.clone()),
                "ICE: user-originated checked core node has an empty source span",
            );
        }
    }

    fn check_global_exists(&mut self, id: DefId, span: &Range<usize>) {
        if !self.env.func_schemes.contains_key(&id) && self.env.class_method_scheme(id).is_none() {
            self.report(
                Some(span.clone()),
                format!(
                    "ICE: global `{}` is missing from core tables",
                    self.env.names.def_display_name(id)
                ),
            );
        }
    }

    fn check_ctor_exists(&mut self, id: CtorId, span: &Range<usize>) {
        if self.env.ctor_table.get(id).is_none() {
            self.report(
                Some(span.clone()),
                format!("ICE: constructor {id:?} is missing from core constructor table"),
            );
        }
    }

    fn check_class_exists(&mut self, id: ClassId, context: &str) {
        if !self.env.class_defs.contains_key(&id) && self.env.class_table.get(id).is_none() {
            self.report(
                None,
                format!("ICE: {context} references unknown class#{}", id.0),
            );
        }
    }

    fn check_predicate_matches(
        &mut self,
        expected: &Predicate,
        actual: &Predicate,
        span: Option<Range<usize>>,
        context: &str,
    ) {
        if !match_predicate_pattern(expected, actual, &mut FxHashMap::default()) {
            self.report(
                span,
                format!(
                    "ICE: {context} predicate mismatch: expected `{}`, got `{}`",
                    self.pretty_predicate_fallback(expected),
                    self.pretty_predicate_fallback(actual)
                ),
            );
        }
    }

    fn pretty_predicate_fallback(&self, pred: &Predicate) -> String {
        let class_name = self
            .env
            .class_table
            .get(pred.class_id)
            .map(|info| self.env.names.symbol_display_name(info.name))
            .or_else(|| {
                self.env
                    .class_defs
                    .get(&pred.class_id)
                    .map(|class_def| class_def.name.clone())
            })
            .unwrap_or_else(|| format!("class#{}", pred.class_id.0));
        let args = pred.args.iter().map(type_fallback_name).collect::<Vec<_>>();

        if args.is_empty() {
            class_name
        } else {
            format!("{} {}", class_name, args.join(" "))
        }
    }

    fn report(&mut self, span: Option<Range<usize>>, message: impl Into<String>) {
        let message = message.into();
        let owner = self.current_owner.clone();
        let owner_name = owner
            .as_ref()
            .map_or_else(|| "<module>".to_string(), |owner| owner.name.clone());
        let mut diagnostic = Diagnostic::error(
            "CORE0001",
            format!("core verification failed in `{owner_name}`"),
        )
        .with_note(message.clone());

        if let (Some(owner), Some(span)) = (owner.as_ref(), span)
            && let Some(file_span) = self.env.file_span(owner.module, &span)
        {
            diagnostic = diagnostic.with_label(Label::primary(file_span, message));
        }

        self.diagnostics.push(diagnostic);
    }
}

fn collect_ev_app_spine(
    expr: &TermExprNode<TcMetadata, Range<usize>>,
) -> (&TermExprNode<TcMetadata, Range<usize>>, Vec<&Ev>) {
    let mut evidence = Vec::new();
    let mut head = expr;

    while let TermExpr::EvApp(inner, ev) = &head.node {
        evidence.push(ev);
        head = inner;
    }

    evidence.reverse();
    (head, evidence)
}

fn global_head_id(expr: &TermExprNode<TcMetadata, Range<usize>>) -> Option<DefId> {
    match &expr.node {
        TermExpr::Var(VarRef::Global(id)) => Some(*id),
        TermExpr::TyApp(inner, _) | TermExpr::Anno(inner, _) => global_head_id(inner),
        _ => None,
    }
}

fn expr_requires_source_span(node: &TermExpr<TcMetadata, Range<usize>>) -> bool {
    matches!(
        node,
        TermExpr::Unit
            | TermExpr::Var(_)
            | TermExpr::Lit(_)
            | TermExpr::Hole
            | TermExpr::Abs(_, _, _)
            | TermExpr::Case(_, _)
            | TermExpr::IfThenElse(_, _, _)
            | TermExpr::Let(_, _, _)
    )
}

fn evidence_ty_vars_for_instance(head: &Predicate, context: &[Predicate]) -> Vec<TyVarId> {
    let mut vars = BTreeSet::new();

    for pred in context {
        free_type_vars_in_predicate(pred, &mut vars);
    }
    free_type_vars_in_predicate(head, &mut vars);

    vars.into_iter().collect()
}

fn free_type_vars_in_predicate(pred: &Predicate, out: &mut BTreeSet<TyVarId>) {
    for arg in &pred.args {
        free_type_vars_in_type(arg, out);
    }
}

fn free_type_vars_in_type(ty: &Ty, out: &mut BTreeSet<TyVarId>) {
    match ty {
        Ty::TyVar(id) => {
            out.insert(*id);
        }
        Ty::TyArr(lhs, rhs) | Ty::TyApp(lhs, rhs) => {
            free_type_vars_in_type(lhs, out);
            free_type_vars_in_type(rhs, out);
        }
        Ty::TyForAll(binders, preds, body) => {
            for pred in preds {
                free_type_vars_in_predicate(pred, out);
            }
            free_type_vars_in_type(body, out);
            for (id, _) in binders {
                out.remove(id);
            }
        }
        Ty::TyUnit | Ty::TyCon(_) | Ty::TyExVar(_) => {}
    }
}

fn subst_from_class_head(params: &[(TyVarId, Kind)], args: &[Ty]) -> FxHashMap<TyVarId, Ty> {
    params
        .iter()
        .zip(args.iter())
        .map(|((param, _), arg)| (*param, arg.clone()))
        .collect()
}

fn substitute_predicate(pred: &Predicate, subst: &FxHashMap<TyVarId, Ty>) -> Predicate {
    Predicate {
        class_id: pred.class_id,
        args: pred
            .args
            .iter()
            .map(|arg| {
                subst
                    .iter()
                    .fold(arg.clone(), |acc, (name, ty)| acc.substitute(*name, ty))
            })
            .collect(),
    }
}

fn match_predicate_pattern(
    pattern: &Predicate,
    actual: &Predicate,
    subst: &mut FxHashMap<TyVarId, Ty>,
) -> bool {
    pattern.class_id == actual.class_id
        && pattern.args.len() == actual.args.len()
        && pattern
            .args
            .iter()
            .zip(actual.args.iter())
            .all(|(pattern, actual)| match_type_pattern(pattern, actual, subst))
}

fn match_type_pattern(pattern: &Ty, actual: &Ty, subst: &mut FxHashMap<TyVarId, Ty>) -> bool {
    match pattern {
        Ty::TyVar(id) => match subst.get(id) {
            Some(existing) => existing == actual,
            None => {
                subst.insert(*id, actual.clone());
                true
            }
        },
        Ty::TyCon(left) => matches!(actual, Ty::TyCon(right) if left == right),
        Ty::TyUnit => matches!(actual, Ty::TyUnit),
        Ty::TyArr(left_arg, left_ret) => match actual {
            Ty::TyArr(right_arg, right_ret) => {
                match_type_pattern(left_arg, right_arg, subst)
                    && match_type_pattern(left_ret, right_ret, subst)
            }
            _ => false,
        },
        Ty::TyApp(left_func, left_arg) => match actual {
            Ty::TyApp(right_func, right_arg) => {
                match_type_pattern(left_func, right_func, subst)
                    && match_type_pattern(left_arg, right_arg, subst)
            }
            _ => false,
        },
        Ty::TyForAll(_, _, body) => match_type_pattern(body, actual, subst),
        Ty::TyExVar(left) => matches!(actual, Ty::TyExVar(right) if left == right),
    }
}

fn type_fallback_name(ty: &Ty) -> String {
    match ty {
        Ty::TyUnit => "()".to_string(),
        Ty::TyVar(id) => format!("t{}", id.0),
        Ty::TyExVar(id) => format!("?{}", id.0),
        Ty::TyCon(id) => format!("T{}", id.0),
        Ty::TyArr(lhs, rhs) => {
            format!("{} -> {}", type_fallback_name(lhs), type_fallback_name(rhs))
        }
        Ty::TyApp(lhs, rhs) => {
            format!("{} {}", type_fallback_name(lhs), type_fallback_name(rhs))
        }
        Ty::TyForAll(_, _, body) => type_fallback_name(body),
    }
}

#[cfg(test)]
mod tests {
    use solveig_ast::ast::{
        ClassInfo, ClassMethodDef, Names, PRIM_TYS, ResolvedModule, TermDefinition, VarRef,
    };

    use super::*;

    fn meta(span: Range<usize>) -> TcMetadata {
        TcMetadata::new(span, Ty::TyUnit)
    }

    fn node(expr: TermExpr<TcMetadata, Range<usize>>) -> TermExprNode<TcMetadata, Range<usize>> {
        TermExprNode::new(expr, meta(0..1))
    }

    fn pred(class_id: ClassId, ty: Ty) -> Predicate {
        Predicate {
            class_id,
            args: vec![ty],
        }
    }

    fn scheme(predicates: Vec<Predicate>) -> Scheme {
        Scheme {
            quantifiers: Vec::new(),
            predicates,
            ty: Ty::TyUnit,
        }
    }

    fn class_def(names: &mut Names, id: ClassId, name: &str) -> ClassDefinition {
        let param = names.fresh_tyvar("a");
        ClassDefinition {
            id,
            name: name.to_string(),
            short_name: name.to_string(),
            params: vec![(param, Kind::Star)],
            superclasses: Vec::new(),
            methods: vec![ClassMethodDef {
                id: names.def_id(&format!("{name}.method")),
                name: format!("{name}.method"),
                short_name: "method".to_string(),
                scheme: scheme(Vec::new()),
                default_body: None,
            }],
        }
    }

    fn module_with_body(
        names: &mut Names,
        body: TermExprNode<TcMetadata, Range<usize>>,
    ) -> CheckedModule {
        module_with_signature_body(names, scheme(Vec::new()), body)
    }

    fn module_with_signature_body(
        names: &mut Names,
        signature: Scheme,
        body: TermExprNode<TcMetadata, Range<usize>>,
    ) -> CheckedModule {
        ResolvedModule {
            data_defs: Vec::new(),
            class_defs: Vec::new(),
            instance_defs: Vec::new(),
            builtin_defs: Vec::new(),
            tycon_table: TyConTable::new(),
            ctor_table: CtorTable::new(),
            class_table: ClassTable::new(),
            tycon_kinds: FxHashMap::default(),
            constructor_types: FxHashMap::default(),
            func_schemes: FxHashMap::default(),
            func_defs: vec![TermDefinition {
                id: names.def_id("main.main"),
                name: "main.main".to_string(),
                short_name: "main".to_string(),
                signature: Some(signature),
                body,
            }],
        }
    }

    fn lint_env(
        names: &mut Names,
        expected_def: DefId,
        expected: Predicate,
        instance_id: InstanceId,
        instance_head: Predicate,
    ) -> CoreLintEnv {
        let mut class_table = ClassTable::new();
        let eq = class_def(names, expected.class_id, "Eq");
        class_table.insert_with_id(
            expected.class_id,
            ClassInfo {
                name: names.symbol("Eq"),
                short_name: names.symbol("Eq"),
                definition: eq.clone(),
            },
        );
        let head_class = class_def(names, instance_head.class_id, "Other");
        class_table.insert_with_id(
            instance_head.class_id,
            ClassInfo {
                name: names.symbol("Other"),
                short_name: names.symbol("Other"),
                definition: head_class.clone(),
            },
        );

        let mut env = CoreLintEnv {
            names: names.clone(),
            class_table,
            ..CoreLintEnv::default()
        }
        .with_file(ModuleId(0), FileId(0));
        env.func_schemes
            .insert(expected_def, scheme(vec![expected]));
        env.class_defs.insert(eq.id, eq);
        env.class_defs.insert(head_class.id, head_class);
        env.insert_instance(instance_id, instance_head, Vec::new());
        env
    }

    fn evidence_app_body(expected_def: DefId, ev: Ev) -> TermExprNode<TcMetadata, Range<usize>> {
        node(TermExpr::EvApp(
            Box::new(TermExprNode::new(
                TermExpr::Var(VarRef::Global(expected_def)),
                meta(0..1),
            )),
            ev,
        ))
    }

    fn notes(diags: &[Diagnostic]) -> String {
        diags
            .iter()
            .flat_map(|diagnostic| diagnostic.notes.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn valid_global_evidence_application_lints_cleanly() {
        let mut names = Names::new();
        let expected_def = names.def_id("test.needsEq");
        let class_id = ClassId(1);
        let instance_id = InstanceId(1);
        let expected = pred(class_id, Ty::TyCon(PRIM_TYS.int));
        let env = lint_env(
            &mut names,
            expected_def,
            expected.clone(),
            instance_id,
            expected,
        );
        let module = module_with_body(
            &mut names,
            evidence_app_body(expected_def, Ev::Inst(instance_id, Vec::new(), Vec::new())),
        );

        assert!(lint_checked_module(&module, &env).is_empty());
    }

    #[test]
    fn evidence_application_predicate_mismatch_is_reported() {
        let mut names = Names::new();
        let expected_def = names.def_id("test.needsEq");
        let eq = ClassId(1);
        let other = ClassId(2);
        let instance_id = InstanceId(1);
        let int = Ty::TyCon(PRIM_TYS.int);
        let env = lint_env(
            &mut names,
            expected_def,
            pred(eq, int.clone()),
            instance_id,
            pred(other, int),
        );
        let module = module_with_body(
            &mut names,
            evidence_app_body(expected_def, Ev::Inst(instance_id, Vec::new(), Vec::new())),
        );
        let diagnostics = lint_checked_module(&module, &env);

        assert!(notes(&diagnostics).contains("predicate mismatch"));
    }

    #[test]
    fn surviving_evidence_placeholder_is_reported() {
        let mut names = Names::new();
        let expected_def = names.def_id("test.needsEq");
        let class_id = ClassId(1);
        let instance_id = InstanceId(1);
        let expected = pred(class_id, Ty::TyCon(PRIM_TYS.int));
        let env = lint_env(
            &mut names,
            expected_def,
            expected.clone(),
            instance_id,
            expected,
        );
        let module = module_with_body(
            &mut names,
            evidence_app_body(expected_def, Ev::Placeholder(EvVarId(99))),
        );
        let diagnostics = lint_checked_module(&module, &env);

        assert!(notes(&diagnostics).contains("placeholder"));
    }

    #[test]
    fn scheme_predicates_without_evidence_binders_are_reported() {
        let mut names = Names::new();
        let expected_def = names.def_id("test.needsEq");
        let class_id = ClassId(1);
        let instance_id = InstanceId(1);
        let expected = pred(class_id, Ty::TyCon(PRIM_TYS.int));
        let env = lint_env(
            &mut names,
            expected_def,
            expected.clone(),
            instance_id,
            expected.clone(),
        );
        let module =
            module_with_signature_body(&mut names, scheme(vec![expected]), node(TermExpr::Unit));
        let diagnostics = lint_checked_module(&module, &env);

        assert!(notes(&diagnostics).contains("scheme/evidence binder arity mismatch"));
    }

    #[test]
    fn empty_source_span_on_user_var_is_reported() {
        let mut names = Names::new();
        let expected_def = names.def_id("test.value");
        let class_id = ClassId(1);
        let instance_id = InstanceId(1);
        let expected = pred(class_id, Ty::TyCon(PRIM_TYS.int));
        let mut env = lint_env(
            &mut names,
            expected_def,
            expected.clone(),
            instance_id,
            expected,
        );
        env.func_schemes.insert(expected_def, scheme(Vec::new()));
        let module = module_with_body(
            &mut names,
            TermExprNode::new(TermExpr::Var(VarRef::Global(expected_def)), meta(0..0)),
        );
        let diagnostics = lint_checked_module(&module, &env);

        assert!(notes(&diagnostics).contains("empty source span"));
    }
}
