//! Source-level inlay hint collection and filtering.

use super::*;

mod instance_equations;

use instance_equations::{
    collect_instance_method_group_inlay_hints, collect_instance_method_group_local_hovers,
};

type CheckedExpr = TermExprNode<TcMetadata, Range<usize>>;

#[derive(Clone, Copy)]
struct LocalHoverContext<'a> {
    session: &'a Session,
    source: &'a str,
    file: FileId,
    tables: PrettyTables<'a>,
    ctor_table: &'a CtorTable,
}

#[derive(Clone, Copy)]
struct InlayGroupContext<'a> {
    session: &'a Session,
    source: &'a str,
    parsed: &'a ParsedModule,
    tables: PrettyTables<'a>,
}

pub(super) fn project_inlay_hints(
    session: &Session,
    project: &Project,
    parsed: &ParsedProject,
    order: &[ModuleName],
    checked: &FxHashMap<ModuleId, CheckedModule>,
    names: &Names,
    modules: &[CheckedModule],
) -> Vec<SourceInlayHint> {
    let (tycon_table, _, class_table) = inlay_display_tables(modules);
    let tables = PrettyTables::new(names, &tycon_table, &class_table);
    let source_map = session.source_map();
    let mut out = Vec::new();

    for module_name in order {
        let Some(module_id) = project.module_table.id(module_name) else {
            continue;
        };
        let Some(checked) = checked.get(&module_id) else {
            continue;
        };
        let parsed = parsed.get(session, module_name);
        let source = &source_map.lookup(parsed.file).src;

        collect_module_inlay_hints(
            session, source, parsed, module_id, checked, tables, &mut out,
        );
    }

    out.sort_by_key(inlay_hint_sort_key);
    out.dedup_by(|left, right| {
        left.file_span() == right.file_span()
            && left.category() == right.category()
            && left.label() == right.label()
    });
    out
}

pub(super) fn project_local_hovers(
    session: &Session,
    project: &Project,
    parsed: &ParsedProject,
    order: &[ModuleName],
    checked: &FxHashMap<ModuleId, CheckedModule>,
    names: &Names,
    modules: &[CheckedModule],
) -> Vec<HoverInfo> {
    let (tycon_table, ctor_table, class_table) = inlay_display_tables(modules);
    let tables = PrettyTables::new(names, &tycon_table, &class_table);
    let source_map = session.source_map();
    let mut out = Vec::new();

    for module_name in order {
        let Some(module_id) = project.module_table.id(module_name) else {
            continue;
        };
        let Some(checked) = checked.get(&module_id) else {
            continue;
        };
        let parsed = parsed.get(session, module_name);
        let source = &source_map.lookup(parsed.file).src;

        let hover_cx = LocalHoverContext {
            session,
            source,
            file: parsed.file,
            tables,
            ctor_table: &ctor_table,
        };
        collect_module_local_hovers(hover_cx, parsed, module_id, checked, &mut out);
    }

    out.sort_by_key(local_hover_sort_key);
    out.dedup_by(|left, right| {
        left.file_span() == right.file_span()
            && left.signature() == right.signature()
            && left.documentation() == right.documentation()
    });
    out
}

pub(super) fn inlay_hints_in_compilation(
    source_map: &SourceMap,
    hints: &[SourceInlayHint],
    file: &Path,
    start: SourcePosition,
    end: SourcePosition,
    encoding: PositionEncoding,
    config: SourceInlayHintConfig,
) -> Vec<SourceInlayHint> {
    let Some(file_id) = source_lookup::source_file_id_in_source_map(source_map, file) else {
        return Vec::new();
    };
    let start = source_map.offset(file_id, start, encoding);
    let end = source_map.offset(file_id, end, encoding);
    let request_span = if start <= end {
        Span::new(start, end)
    } else {
        Span::new(end, start)
    };

    hints
        .iter()
        .filter(|hint| hint.file_span().file == file_id)
        .filter(|hint| config.enables(hint.category()))
        .filter(|hint| span_contains_position(request_span, hint.file_span().span.start))
        .cloned()
        .collect()
}

fn collect_module_inlay_hints(
    session: &Session,
    source: &str,
    parsed: &ParsedModule,
    module_id: ModuleId,
    checked: &CheckedModule,
    tables: PrettyTables<'_>,
    out: &mut Vec<SourceInlayHint>,
) {
    let signatures = parsed
        .defs
        .iter()
        .filter_map(|defn| match &defn.0 {
            SurfaceDefinition::SigDef { name, .. } => Some(name.0),
            _ => None,
        })
        .collect::<FxHashSet<_>>();
    let checked_defs = checked
        .func_defs
        .iter()
        .map(|def| (def.id, def))
        .collect::<FxHashMap<_, _>>();

    let function_groups = function_equation_group_indices(parsed).unwrap_or_default();

    for group in function_groups {
        let Some(first_index) = group.first().copied() else {
            continue;
        };
        let Some(first_def) = parsed.defs.get(first_index) else {
            continue;
        };
        let SurfaceDefinition::FunDef { name, params, .. } = &first_def.0 else {
            continue;
        };
        let def_id = DefId {
            module: module_id,
            name: name.0,
        };
        let Some(checked_def) = checked_defs.get(&def_id).copied() else {
            continue;
        };

        if !signatures.contains(&name.0)
            && let Some(scheme) = checked.func_schemes.get(&def_id)
        {
            push_inferred_signature_hint(parsed.file, name, scheme, tables, out);
        }

        collect_function_group_inlay_hints(
            InlayGroupContext {
                session,
                source,
                parsed,
                tables,
            },
            &group,
            params.len(),
            checked_def,
            out,
        );
    }

    let mut checked_instances = checked.instance_defs.iter();
    for defn in &parsed.defs {
        let SurfaceDefinition::InstanceDef { methods, .. } = &defn.0 else {
            continue;
        };
        let Some(checked_instance) = checked_instances.next() else {
            continue;
        };

        let method_groups =
            instance_method_equation_group_indices(parsed.file, methods).unwrap_or_default();
        for group in method_groups {
            let Some(first_method) = group.first().and_then(|index| methods.get(*index)) else {
                continue;
            };
            let method_name = session.str(first_method.name.0);
            let Some(checked_method) = checked_instance
                .methods
                .iter()
                .find(|checked_method| checked_method.name == *method_name)
            else {
                continue;
            };
            drop(method_name);

            collect_instance_method_group_inlay_hints(
                InlayGroupContext {
                    session,
                    source,
                    parsed,
                    tables,
                },
                methods,
                &group,
                checked_method,
                out,
            );
        }
    }
}

fn collect_module_local_hovers(
    cx: LocalHoverContext<'_>,
    parsed: &ParsedModule,
    module_id: ModuleId,
    checked: &CheckedModule,
    out: &mut Vec<HoverInfo>,
) {
    let session = cx.session;
    let checked_defs = checked
        .func_defs
        .iter()
        .map(|def| (def.id, def))
        .collect::<FxHashMap<_, _>>();

    let function_groups = function_equation_group_indices(parsed).unwrap_or_default();

    for group in function_groups {
        let Some(first_index) = group.first().copied() else {
            continue;
        };
        let Some(first_def) = parsed.defs.get(first_index) else {
            continue;
        };
        let SurfaceDefinition::FunDef { name, params, .. } = &first_def.0 else {
            continue;
        };
        let def_id = DefId {
            module: module_id,
            name: name.0,
        };
        let Some(checked_def) = checked_defs.get(&def_id).copied() else {
            continue;
        };
        collect_function_group_local_hovers(cx, parsed, &group, params.len(), checked_def, out);
    }

    let mut checked_instances = checked.instance_defs.iter();
    for defn in &parsed.defs {
        let SurfaceDefinition::InstanceDef { methods, .. } = &defn.0 else {
            continue;
        };
        let Some(checked_instance) = checked_instances.next() else {
            continue;
        };

        let method_groups =
            instance_method_equation_group_indices(parsed.file, methods).unwrap_or_default();
        for group in method_groups {
            let Some(first_method) = group.first().and_then(|index| methods.get(*index)) else {
                continue;
            };
            let method_name = session.str(first_method.name.0);
            let Some(checked_method) = checked_instance
                .methods
                .iter()
                .find(|checked_method| checked_method.name == *method_name)
            else {
                continue;
            };
            drop(method_name);

            collect_instance_method_group_local_hovers(cx, methods, &group, checked_method, out);
        }
    }
}

fn push_inferred_signature_hint(
    file: FileId,
    name: &Spanned<Symbol>,
    scheme: &Scheme,
    tables: PrettyTables<'_>,
    out: &mut Vec<SourceInlayHint>,
) {
    out.push(SourceInlayHint::new(
        FileSpan::new(file, Span::new(name.1.end, name.1.end)),
        format!(
            ": {}",
            source_signature::pretty_source_scheme(scheme, tables)
        ),
        SourceInlayHintKind::Type,
        SourceInlayHintCategory::InferredSignature,
    ));
}

fn collect_function_group_inlay_hints(
    cx: InlayGroupContext<'_>,
    group: &[usize],
    param_count: usize,
    checked_def: &TermDefinition<TcMetadata, Range<usize>>,
    out: &mut Vec<SourceInlayHint>,
) {
    let checked_body = checked_body_without_function_params(&checked_def.body, param_count);

    if group.len() == 1
        && let Some(first_index) = group.first().copied()
        && let Some(defn) = cx.parsed.defs.get(first_index)
        && let SurfaceDefinition::FunDef { body, params, .. } = &defn.0
        && let Some(checked_body) = simple_variable_params(params).and_then(|simple_params| {
            collect_function_param_hints(
                cx.session,
                cx.parsed.file,
                &simple_params,
                &checked_def.body,
                cx.tables,
                out,
            )
        })
    {
        collect_expr_inlay_hints(
            cx.session,
            cx.parsed.file,
            body,
            checked_body,
            cx.tables,
            out,
        );
        collect_textual_let_hints(cx.parsed.file, cx.source, checked_body, cx.tables, out);
        return;
    }

    let TermExpr::Match(_, arms) = &checked_body.node else {
        return;
    };

    for (def_index, checked_arm) in group.iter().zip(arms) {
        let Some(defn) = cx.parsed.defs.get(*def_index) else {
            continue;
        };
        let SurfaceDefinition::FunDef { body, .. } = &defn.0 else {
            continue;
        };
        collect_expr_inlay_hints(
            cx.session,
            cx.parsed.file,
            body,
            &checked_arm.node.expr,
            cx.tables,
            out,
        );
        collect_textual_let_hints(
            cx.parsed.file,
            cx.source,
            &checked_arm.node.expr,
            cx.tables,
            out,
        );
    }
}

fn simple_variable_params(params: &[Spanned<SurfacePattern>]) -> Option<Vec<Spanned<Symbol>>> {
    params
        .iter()
        .map(|param| match param {
            (SurfacePattern::Var(symbol), span, cst_id) => Some((*symbol, *span, *cst_id)),
            _ => None,
        })
        .collect()
}

fn collect_function_param_hints<'a>(
    session: &Session,
    file: FileId,
    params: &[Spanned<Symbol>],
    checked: &'a TermExprNode<TcMetadata, Range<usize>>,
    tables: PrettyTables<'_>,
    out: &mut Vec<SourceInlayHint>,
) -> Option<&'a TermExprNode<TcMetadata, Range<usize>>> {
    let mut current = checked;

    for param in params {
        current = strip_checked_wrappers(current);
        let TermExpr::Abs(checked_name, checked_type, body) = &current.node else {
            return None;
        };
        if checked_name != &*session.str(param.0) {
            return None;
        }

        if let Some(ty) = checked_type {
            out.push(SourceInlayHint::new(
                FileSpan::new(file, Span::new(param.1.end, param.1.end)),
                format!(": {}", ty.pretty_with_tables(tables)),
                SourceInlayHintKind::Type,
                SourceInlayHintCategory::FunctionParameterType,
            ));
        }

        current = body;
    }

    Some(strip_checked_wrappers(current))
}

fn collect_function_param_hovers<'a>(
    session: &Session,
    file: FileId,
    params: &[Spanned<Symbol>],
    checked: &'a TermExprNode<TcMetadata, Range<usize>>,
    tables: PrettyTables<'_>,
    binder_facts: &mut Vec<(Symbol, String, Ty)>,
    out: &mut Vec<HoverInfo>,
) -> Option<&'a TermExprNode<TcMetadata, Range<usize>>> {
    let mut current = checked;

    for param in params {
        current = strip_checked_wrappers(current);
        let TermExpr::Abs(checked_name, checked_type, body) = &current.node else {
            return None;
        };
        if checked_name != &*session.str(param.0) {
            return None;
        }

        if let Some(ty) = checked_type {
            push_local_hover(file, param.1, checked_name, ty, tables, out);
            binder_facts.push((param.0, checked_name.clone(), ty.clone()));
        }

        current = body;
    }

    Some(strip_checked_wrappers(current))
}

fn collect_expr_inlay_hints(
    session: &Session,
    file: FileId,
    surface: &Spanned<SurfaceExpr>,
    checked: &TermExprNode<TcMetadata, Range<usize>>,
    tables: PrettyTables<'_>,
    out: &mut Vec<SourceInlayHint>,
) {
    let checked = strip_checked_wrappers(checked);

    match (&surface.0, &checked.node) {
        (SurfaceExpr::Abs(params, body), _) => {
            if let Some(checked_body) =
                collect_lambda_param_hints(session, file, params, checked, tables, out)
            {
                collect_expr_inlay_hints(session, file, body, checked_body, tables, out);
            }
        }
        (
            SurfaceExpr::Let((SurfacePattern::Var(name), name_span, _), binder, body),
            TermExpr::Let(checked_name, checked_binder, checked_body),
        ) if checked_name == &*session.str(*name) => {
            push_let_binding_hint(file, name_span.end, checked_binder, tables, out);
            collect_expr_inlay_hints(session, file, binder, checked_binder, tables, out);
            collect_expr_inlay_hints(session, file, body, checked_body, tables, out);
        }
        (SurfaceExpr::Let(pattern, binder, body), TermExpr::Case(checked_binder, checked_arms))
            if checked_arms.len() == 1 =>
        {
            if let Some(checked_arm) = checked_arms.first() {
                push_let_binding_hint(file, pattern.1.end, checked_binder, tables, out);
                collect_expr_inlay_hints(session, file, binder, checked_binder, tables, out);
                collect_expr_inlay_hints(session, file, body, &checked_arm.node.expr, tables, out);
            }
        }
        (SurfaceExpr::App(surface_func, surface_arg), TermExpr::App(checked_func, checked_arg)) => {
            collect_expr_inlay_hints(session, file, surface_func, checked_func, tables, out);
            collect_expr_inlay_hints(session, file, surface_arg, checked_arg, tables, out);
        }
        (SurfaceExpr::Ann(value, _), TermExpr::Anno(checked_value, _)) => {
            collect_expr_inlay_hints(session, file, value, checked_value, tables, out);
        }
        (SurfaceExpr::Ann(value, _), _) => {
            collect_expr_inlay_hints(session, file, value, checked, tables, out);
        }
        (
            SurfaceExpr::IfThenElse(surface_cond, surface_then, surface_else),
            TermExpr::IfThenElse(checked_cond, checked_then, checked_else),
        ) => {
            collect_expr_inlay_hints(session, file, surface_cond, checked_cond, tables, out);
            collect_expr_inlay_hints(session, file, surface_then, checked_then, tables, out);
            collect_expr_inlay_hints(session, file, surface_else, checked_else, tables, out);
        }
        (
            SurfaceExpr::Case(surface_scrutinee, surface_arms),
            TermExpr::Case(checked_scrutinee, checked_arms),
        ) => {
            collect_expr_inlay_hints(
                session,
                file,
                surface_scrutinee,
                checked_scrutinee,
                tables,
                out,
            );
            for (surface_arm, checked_arm) in surface_arms.iter().zip(checked_arms) {
                collect_expr_inlay_hints(
                    session,
                    file,
                    &surface_arm.0.expr,
                    &checked_arm.node.expr,
                    tables,
                    out,
                );
            }
        }
        (
            SurfaceExpr::OperatorApply(surface_lhs, _, surface_rhs),
            TermExpr::App(checked_func, checked_rhs),
        ) => {
            if let TermExpr::App(_, checked_lhs) = &strip_checked_wrappers(checked_func).node {
                collect_expr_inlay_hints(session, file, surface_lhs, checked_lhs, tables, out);
            }
            collect_expr_inlay_hints(session, file, surface_rhs, checked_rhs, tables, out);
        }
        (SurfaceExpr::UnaryOperator(_, surface_rhs), TermExpr::App(_, checked_rhs)) => {
            collect_expr_inlay_hints(session, file, surface_rhs, checked_rhs, tables, out);
        }
        _ => {}
    }
}

fn collect_function_group_local_hovers(
    cx: LocalHoverContext<'_>,
    parsed: &ParsedModule,
    group: &[usize],
    param_count: usize,
    checked_def: &TermDefinition<TcMetadata, Range<usize>>,
    out: &mut Vec<HoverInfo>,
) {
    let checked_body = checked_body_without_function_params(&checked_def.body, param_count);

    if group.len() == 1
        && let Some(first_index) = group.first().copied()
        && let Some(defn) = parsed.defs.get(first_index)
        && let SurfaceDefinition::FunDef { body, params, .. } = &defn.0
    {
        let mut param_facts = Vec::new();
        if let Some(checked_body) = simple_variable_params(params).and_then(|simple_params| {
            collect_function_param_hovers(
                cx.session,
                cx.file,
                &simple_params,
                &checked_def.body,
                cx.tables,
                &mut param_facts,
                out,
            )
        }) {
            for (symbol, name, ty) in &param_facts {
                push_expr_reference_hovers(cx.file, body, *symbol, name, ty, cx.tables, out);
            }
            collect_expr_local_hovers(cx, body, checked_body, out);
            return;
        }
    }

    let TermExpr::Match(scrutinees, arms) = &checked_body.node else {
        return;
    };

    for (def_index, checked_arm) in group.iter().zip(arms) {
        let Some(defn) = parsed.defs.get(*def_index) else {
            continue;
        };
        let SurfaceDefinition::FunDef { params, body, .. } = &defn.0 else {
            continue;
        };

        let mut pattern_facts = Vec::new();
        {
            let mut sink = PatternLocalHoverSink {
                binder_facts: &mut pattern_facts,
                out,
            };
            for ((surface_param, checked_pattern), checked_scrutinee) in params
                .iter()
                .zip(&checked_arm.node.patterns)
                .zip(scrutinees)
            {
                collect_pattern_local_hovers(
                    cx.file,
                    surface_param,
                    checked_pattern,
                    &checked_scrutinee.anno.inferred_type,
                    cx.tables,
                    cx.ctor_table,
                    &mut sink,
                );
            }
        }

        for (symbol, name, ty) in &pattern_facts {
            push_expr_reference_hovers(cx.file, body, *symbol, name, ty, cx.tables, out);
        }
        collect_expr_local_hovers(cx, body, &checked_arm.node.expr, out);
    }
}

fn collect_expr_local_hovers(
    cx: LocalHoverContext<'_>,
    surface: &Spanned<SurfaceExpr>,
    checked: &TermExprNode<TcMetadata, Range<usize>>,
    out: &mut Vec<HoverInfo>,
) {
    let session = cx.session;
    let file = cx.file;
    let tables = cx.tables;
    let checked = strip_checked_wrappers(checked);
    push_grouped_expr_hover(file, cx.source, surface, checked, tables, out);

    match (&surface.0, &checked.node) {
        (SurfaceExpr::Abs(params, body), _) => {
            let mut param_facts = Vec::new();
            if let Some(checked_body) =
                collect_lambda_param_hovers(cx, params, checked, &mut param_facts, out)
            {
                for (symbol, name, ty) in &param_facts {
                    push_expr_reference_hovers(file, body, *symbol, name, ty, tables, out);
                }
                collect_expr_local_hovers(cx, body, checked_body, out);
            }
        }
        (
            SurfaceExpr::Let((SurfacePattern::Var(name), name_span, _), binder, body),
            TermExpr::Let(checked_name, checked_binder, checked_body),
        ) if checked_name == &*session.str(*name) => {
            push_local_hover(
                file,
                *name_span,
                checked_name,
                &checked_binder.anno.inferred_type,
                tables,
                out,
            );
            push_expr_reference_hovers(
                file,
                body,
                *name,
                checked_name,
                &checked_binder.anno.inferred_type,
                tables,
                out,
            );
            collect_expr_local_hovers(cx, binder, checked_binder, out);
            collect_expr_local_hovers(cx, body, checked_body, out);
        }
        (SurfaceExpr::Let(pattern, binder, body), TermExpr::Case(checked_binder, checked_arms))
            if checked_arms.len() == 1 =>
        {
            let Some(checked_arm) = checked_arms.first() else {
                return;
            };
            let mut pattern_facts = Vec::new();
            {
                let mut sink = PatternLocalHoverSink {
                    binder_facts: &mut pattern_facts,
                    out,
                };
                collect_pattern_local_hovers(
                    file,
                    pattern,
                    &checked_arm.node.pattern,
                    &checked_binder.anno.inferred_type,
                    tables,
                    cx.ctor_table,
                    &mut sink,
                );
            }
            for (symbol, name, ty) in &pattern_facts {
                push_expr_reference_hovers(file, body, *symbol, name, ty, tables, out);
            }
            collect_expr_local_hovers(cx, binder, checked_binder, out);
            collect_expr_local_hovers(cx, body, &checked_arm.node.expr, out);
        }
        (SurfaceExpr::App(surface_func, surface_arg), TermExpr::App(checked_func, checked_arg)) => {
            collect_expr_local_hovers(cx, surface_func, checked_func, out);
            collect_expr_local_hovers(cx, surface_arg, checked_arg, out);
        }
        (SurfaceExpr::Ann(value, _), TermExpr::Anno(checked_value, _)) => {
            collect_expr_local_hovers(cx, value, checked_value, out);
        }
        (SurfaceExpr::Ann(value, _), _) => {
            collect_expr_local_hovers(cx, value, checked, out);
        }
        (
            SurfaceExpr::IfThenElse(surface_cond, surface_then, surface_else),
            TermExpr::IfThenElse(checked_cond, checked_then, checked_else),
        ) => {
            collect_expr_local_hovers(cx, surface_cond, checked_cond, out);
            collect_expr_local_hovers(cx, surface_then, checked_then, out);
            collect_expr_local_hovers(cx, surface_else, checked_else, out);
        }
        (
            SurfaceExpr::Case(surface_scrutinee, surface_arms),
            TermExpr::Case(checked_scrutinee, checked_arms),
        ) => {
            collect_expr_local_hovers(cx, surface_scrutinee, checked_scrutinee, out);
            for (surface_arm, checked_arm) in surface_arms.iter().zip(checked_arms) {
                let mut pattern_facts = Vec::new();
                {
                    let mut sink = PatternLocalHoverSink {
                        binder_facts: &mut pattern_facts,
                        out,
                    };
                    collect_pattern_local_hovers(
                        file,
                        &surface_arm.0.pattern,
                        &checked_arm.node.pattern,
                        &checked_scrutinee.anno.inferred_type,
                        tables,
                        cx.ctor_table,
                        &mut sink,
                    );
                }
                for (symbol, name, ty) in &pattern_facts {
                    push_expr_reference_hovers(
                        file,
                        &surface_arm.0.expr,
                        *symbol,
                        name,
                        ty,
                        tables,
                        out,
                    );
                }
                collect_expr_local_hovers(cx, &surface_arm.0.expr, &checked_arm.node.expr, out);
            }
        }
        (SurfaceExpr::Do(statements), _) => {
            collect_do_local_hovers(cx, statements, checked, out);
        }
        (
            SurfaceExpr::OperatorApply(surface_lhs, _, surface_rhs),
            TermExpr::App(checked_func, checked_rhs),
        ) => {
            if let TermExpr::App(_, checked_lhs) = &strip_checked_wrappers(checked_func).node {
                collect_expr_local_hovers(cx, surface_lhs, checked_lhs, out);
            }
            collect_expr_local_hovers(cx, surface_rhs, checked_rhs, out);
        }
        (SurfaceExpr::UnaryOperator(_, surface_rhs), TermExpr::App(_, checked_rhs)) => {
            collect_expr_local_hovers(cx, surface_rhs, checked_rhs, out);
        }
        _ => {}
    }
}

fn push_grouped_expr_hover(
    file: FileId,
    source: &str,
    surface: &Spanned<SurfaceExpr>,
    checked: &TermExprNode<TcMetadata, Range<usize>>,
    tables: PrettyTables<'_>,
    out: &mut Vec<HoverInfo>,
) {
    if !source_span_is_grouped_expr(source, surface.1) {
        return;
    }

    out.push(HoverInfo::new(
        file,
        surface.1.to_usize_range(),
        checked.anno.inferred_type.pretty_with_tables(tables),
        None,
    ));
}

fn source_span_is_grouped_expr(source: &str, span: Span) -> bool {
    let Some(text) = source.get(span.to_usize_range()) else {
        return false;
    };
    let text = text.trim();

    (text.starts_with('(') && text.ends_with(')')) || (text.starts_with('[') && text.ends_with(']'))
}

fn collect_lambda_param_hints<'a>(
    session: &Session,
    file: FileId,
    params: &[(Spanned<SurfacePattern>, Option<Spanned<SurfaceTy>>)],
    checked: &'a TermExprNode<TcMetadata, Range<usize>>,
    tables: PrettyTables<'_>,
    out: &mut Vec<SourceInlayHint>,
) -> Option<&'a TermExprNode<TcMetadata, Range<usize>>> {
    let mut current = checked;

    for (param, source_annotation) in params {
        current = strip_checked_wrappers(current);
        let TermExpr::Abs(checked_name, checked_type, body) = &current.node else {
            return None;
        };
        match param.0 {
            SurfacePattern::Var(symbol) if checked_name == &*session.str(symbol) => {}
            SurfacePattern::Wild if checked_name == "_" => {}
            SurfacePattern::Var(_) | SurfacePattern::Wild => return None,
            _ => {}
        }

        if source_annotation.is_none()
            && let Some(ty) = checked_type
        {
            out.push(SourceInlayHint::new(
                FileSpan::new(file, Span::new(param.1.end, param.1.end)),
                format!(": {}", ty.pretty_with_tables(tables)),
                SourceInlayHintKind::Type,
                SourceInlayHintCategory::LambdaParameterType,
            ));
        }

        current = if matches!(param.0, SurfacePattern::Var(_) | SurfacePattern::Wild) {
            body
        } else {
            let TermExpr::Case(_, arms) = &strip_checked_wrappers(body).node else {
                return None;
            };
            let [arm] = arms.as_slice() else {
                return None;
            };
            &arm.node.expr
        };
    }

    Some(strip_checked_wrappers(current))
}

fn collect_lambda_param_hovers<'a>(
    cx: LocalHoverContext<'_>,
    params: &[(Spanned<SurfacePattern>, Option<Spanned<SurfaceTy>>)],
    checked: &'a TermExprNode<TcMetadata, Range<usize>>,
    binder_facts: &mut Vec<(Symbol, String, Ty)>,
    out: &mut Vec<HoverInfo>,
) -> Option<&'a TermExprNode<TcMetadata, Range<usize>>> {
    let session = cx.session;
    let file = cx.file;
    let tables = cx.tables;
    let mut current = checked;

    for (param, _) in params {
        current = strip_checked_wrappers(current);
        let TermExpr::Abs(checked_name, checked_type, body) = &current.node else {
            return None;
        };
        match &param.0 {
            SurfacePattern::Var(symbol) if checked_name == &*session.str(*symbol) => {
                if let Some(ty) = checked_type {
                    push_local_hover(file, param.1, checked_name, ty, tables, out);
                    binder_facts.push((*symbol, checked_name.clone(), ty.clone()));
                }
                current = body;
            }
            SurfacePattern::Wild if checked_name == "_" => {
                if let Some(ty) = checked_type {
                    push_local_hover(file, param.1, checked_name, ty, tables, out);
                }
                current = body;
            }
            SurfacePattern::Var(_) | SurfacePattern::Wild => return None,
            _ => {
                let Some(ty) = checked_type else {
                    return None;
                };
                let TermExpr::Case(_, arms) = &strip_checked_wrappers(body).node else {
                    return None;
                };
                let [arm] = arms.as_slice() else {
                    return None;
                };
                let mut sink = PatternLocalHoverSink { binder_facts, out };
                collect_pattern_local_hovers(
                    file,
                    param,
                    &arm.node.pattern,
                    ty,
                    tables,
                    cx.ctor_table,
                    &mut sink,
                );
                current = &arm.node.expr;
            }
        }
    }

    Some(strip_checked_wrappers(current))
}

fn collect_do_local_hovers(
    cx: LocalHoverContext<'_>,
    statements: &[Spanned<SurfaceDoStmt>],
    checked: &TermExprNode<TcMetadata, Range<usize>>,
    out: &mut Vec<HoverInfo>,
) {
    let session = cx.session;
    let file = cx.file;
    let tables = cx.tables;
    let Some((first, rest)) = statements.split_first() else {
        return;
    };

    if rest.is_empty() {
        if let SurfaceDoStmt::Expr(expr) = &first.0 {
            collect_expr_local_hovers(cx, expr, checked, out);
        }
        return;
    }

    let checked = strip_checked_wrappers(checked);
    match &first.0 {
        SurfaceDoStmt::Expr(expr) => {
            if let Some((checked_expr, checked_rest)) = checked_binary_application_args(checked) {
                collect_expr_local_hovers(cx, expr, checked_expr, out);
                collect_do_local_hovers(cx, rest, checked_rest, out);
            }
        }
        SurfaceDoStmt::Bind { pattern, expr } => {
            if let Some((checked_expr, checked_continuation)) =
                checked_binary_application_args(checked)
            {
                collect_expr_local_hovers(cx, expr, checked_expr, out);

                let checked_continuation = strip_checked_wrappers(checked_continuation);
                if let TermExpr::Abs(checked_name, checked_type, checked_body) =
                    &checked_continuation.node
                {
                    match &pattern.0 {
                        SurfacePattern::Var(symbol) if checked_name == &*session.str(*symbol) => {
                            if let Some(ty) = checked_type {
                                push_local_hover(file, pattern.1, checked_name, ty, tables, out);
                                push_do_binder_reference_hovers(
                                    file,
                                    rest,
                                    *symbol,
                                    checked_name,
                                    ty,
                                    tables,
                                    out,
                                );
                            }
                            collect_do_local_hovers(cx, rest, checked_body, out);
                        }
                        SurfacePattern::Wild if checked_name == "_" => {
                            if let Some(ty) = checked_type {
                                push_local_hover(file, pattern.1, checked_name, ty, tables, out);
                            }
                            collect_do_local_hovers(cx, rest, checked_body, out);
                        }
                        SurfacePattern::Var(_) | SurfacePattern::Wild => {}
                        _ => {
                            let Some(ty) = checked_type else {
                                return;
                            };
                            let TermExpr::Case(_, arms) =
                                &strip_checked_wrappers(checked_body).node
                            else {
                                return;
                            };
                            let [arm] = arms.as_slice() else {
                                return;
                            };
                            let mut facts = Vec::new();
                            {
                                let mut sink = PatternLocalHoverSink {
                                    binder_facts: &mut facts,
                                    out,
                                };
                                collect_pattern_local_hovers(
                                    file,
                                    pattern,
                                    &arm.node.pattern,
                                    ty,
                                    tables,
                                    cx.ctor_table,
                                    &mut sink,
                                );
                            }
                            for (symbol, name, ty) in facts {
                                push_do_binder_reference_hovers(
                                    file, rest, symbol, &name, &ty, tables, out,
                                );
                            }
                            collect_do_local_hovers(cx, rest, &arm.node.expr, out);
                        }
                    }
                }
            }
        }
        SurfaceDoStmt::Let { pattern, expr } => match (&pattern.0, &checked.node) {
            (
                SurfacePattern::Var(symbol),
                TermExpr::Let(checked_name, checked_binder, checked_body),
            ) if checked_name == &*session.str(*symbol) => {
                push_local_hover(
                    file,
                    pattern.1,
                    checked_name,
                    &checked_binder.anno.inferred_type,
                    tables,
                    out,
                );
                push_do_binder_reference_hovers(
                    file,
                    rest,
                    *symbol,
                    checked_name,
                    &checked_binder.anno.inferred_type,
                    tables,
                    out,
                );
                collect_expr_local_hovers(cx, expr, checked_binder, out);
                collect_do_local_hovers(cx, rest, checked_body, out);
            }
            (_, TermExpr::Case(checked_binder, arms)) if arms.len() == 1 => {
                let Some(arm) = arms.first() else {
                    return;
                };
                collect_expr_local_hovers(cx, expr, checked_binder, out);
                let mut facts = Vec::new();
                {
                    let mut sink = PatternLocalHoverSink {
                        binder_facts: &mut facts,
                        out,
                    };
                    collect_pattern_local_hovers(
                        file,
                        pattern,
                        &arm.node.pattern,
                        &checked_binder.anno.inferred_type,
                        tables,
                        cx.ctor_table,
                        &mut sink,
                    );
                }
                for (symbol, name, ty) in facts {
                    push_do_binder_reference_hovers(file, rest, symbol, &name, &ty, tables, out);
                }
                collect_do_local_hovers(cx, rest, &arm.node.expr, out);
            }
            _ => {}
        },
    }
}

fn checked_binary_application_args(checked: &CheckedExpr) -> Option<(&CheckedExpr, &CheckedExpr)> {
    let checked = strip_checked_wrappers(checked);
    let TermExpr::App(func, second) = &checked.node else {
        return None;
    };
    let func = strip_checked_wrappers(func);
    let TermExpr::App(_, first) = &func.node else {
        return None;
    };

    Some((first, second))
}

fn push_do_binder_reference_hovers(
    file: FileId,
    statements: &[Spanned<SurfaceDoStmt>],
    symbol: Symbol,
    name: &str,
    ty: &Ty,
    tables: PrettyTables<'_>,
    out: &mut Vec<HoverInfo>,
) {
    for statement in statements {
        match &statement.0 {
            SurfaceDoStmt::Expr(expr) => {
                push_expr_reference_hovers(file, expr, symbol, name, ty, tables, out);
            }
            SurfaceDoStmt::Bind { pattern, expr } | SurfaceDoStmt::Let { pattern, expr } => {
                push_expr_reference_hovers(file, expr, symbol, name, ty, tables, out);
                if pattern_binds_symbol(pattern, symbol) {
                    break;
                }
            }
        }
    }
}

fn push_expr_reference_hovers(
    file: FileId,
    expr: &Spanned<SurfaceExpr>,
    symbol: Symbol,
    name: &str,
    ty: &Ty,
    tables: PrettyTables<'_>,
    out: &mut Vec<HoverInfo>,
) {
    match &expr.0 {
        SurfaceExpr::Var(name_ref) if name_ref.0.segments.as_slice() == [symbol] => {
            push_local_hover(file, name_ref.1, name, ty, tables, out);
        }
        SurfaceExpr::App(lhs, rhs) | SurfaceExpr::OperatorApply(lhs, _, rhs) => {
            push_expr_reference_hovers(file, lhs, symbol, name, ty, tables, out);
            push_expr_reference_hovers(file, rhs, symbol, name, ty, tables, out);
        }
        SurfaceExpr::Abs(params, body) => {
            if !params
                .iter()
                .any(|(pattern, _)| pattern_binds_symbol(pattern, symbol))
            {
                push_expr_reference_hovers(file, body, symbol, name, ty, tables, out);
            }
        }
        SurfaceExpr::Ann(value, _)
        | SurfaceExpr::UnaryOperator(_, value)
        | SurfaceExpr::OperatorLeftSection(value, _)
        | SurfaceExpr::OperatorRightSection(_, value) => {
            push_expr_reference_hovers(file, value, symbol, name, ty, tables, out);
        }
        SurfaceExpr::Let(binder, value, body) => {
            push_expr_reference_hovers(file, value, symbol, name, ty, tables, out);
            if !pattern_binds_symbol(binder, symbol) {
                push_expr_reference_hovers(file, body, symbol, name, ty, tables, out);
            }
        }
        SurfaceExpr::Case(scrutinee, arms) => {
            push_expr_reference_hovers(file, scrutinee, symbol, name, ty, tables, out);
            for arm in arms {
                if !pattern_binds_symbol(&arm.0.pattern, symbol) {
                    push_expr_reference_hovers(file, &arm.0.expr, symbol, name, ty, tables, out);
                }
            }
        }
        SurfaceExpr::Do(statements) => {
            push_do_binder_reference_hovers(file, statements, symbol, name, ty, tables, out);
        }
        SurfaceExpr::IfThenElse(cond, then_expr, else_expr) => {
            push_expr_reference_hovers(file, cond, symbol, name, ty, tables, out);
            push_expr_reference_hovers(file, then_expr, symbol, name, ty, tables, out);
            push_expr_reference_hovers(file, else_expr, symbol, name, ty, tables, out);
        }
        SurfaceExpr::OpSeq(first, rest) => {
            push_expr_reference_hovers(file, &first.expr, symbol, name, ty, tables, out);
            for segment in rest {
                push_expr_reference_hovers(file, &segment.rhs.expr, symbol, name, ty, tables, out);
            }
        }
        SurfaceExpr::Unit
        | SurfaceExpr::Literal(_)
        | SurfaceExpr::Hole
        | SurfaceExpr::Con(_)
        | SurfaceExpr::Var(_) => {}
    }
}

fn pattern_binds_symbol(pattern: &Spanned<SurfacePattern>, symbol: Symbol) -> bool {
    match &pattern.0 {
        SurfacePattern::Var(candidate) => *candidate == symbol,
        SurfacePattern::Con(_, args) => args.iter().any(|arg| pattern_binds_symbol(arg, symbol)),
        SurfacePattern::Lit(_) | SurfacePattern::Wild => false,
    }
}

struct PatternLocalHoverSink<'a> {
    binder_facts: &'a mut Vec<(Symbol, String, Ty)>,
    out: &'a mut Vec<HoverInfo>,
}

fn collect_pattern_local_hovers(
    file: FileId,
    surface: &Spanned<SurfacePattern>,
    pattern: &PatternNode<Range<usize>>,
    expected_type: &Ty,
    tables: PrettyTables<'_>,
    ctor_table: &CtorTable,
    sink: &mut PatternLocalHoverSink<'_>,
) {
    match (&surface.0, &pattern.node) {
        (SurfacePattern::Var(symbol), Pattern::Var(name)) => {
            push_local_hover(file, surface.1, name, expected_type, tables, sink.out);
            sink.binder_facts
                .push((*symbol, name.clone(), expected_type.clone()));
        }
        (SurfacePattern::Wild, Pattern::Wild) => {
            push_local_hover(file, surface.1, "_", expected_type, tables, sink.out);
        }
        (SurfacePattern::Con(_, surface_args), Pattern::Con(ctor_id, args)) => {
            if tuple_ctor_arity_from_name(&tables.names.ctor_display_name(*ctor_id)).is_some() {
                sink.out.push(HoverInfo::new(
                    file,
                    surface.1.to_usize_range(),
                    expected_type.pretty_with_tables(tables),
                    None,
                ));
            }
            let field_types = ctor_table
                .get(*ctor_id)
                .and_then(|info| {
                    specialized_constructor_field_types(&info.full_type, expected_type, args.len())
                })
                .unwrap_or_default();

            for ((surface_arg, arg), field_type) in
                surface_args.iter().zip(args).zip(field_types.iter())
            {
                collect_pattern_local_hovers(
                    file,
                    surface_arg,
                    arg,
                    field_type,
                    tables,
                    ctor_table,
                    sink,
                );
            }
        }
        _ => {}
    }
}

fn specialized_constructor_field_types(
    constructor_type: &Ty,
    expected_result_type: &Ty,
    arity: usize,
) -> Option<Vec<Ty>> {
    let (_, _, mut current) = constructor_type.peel_scheme_like();
    let mut fields = Vec::with_capacity(arity);

    for _ in 0..arity {
        let Ty::TyArr(field_type, rest) = current else {
            return None;
        };
        fields.push(*field_type);
        current = *rest;
    }

    let mut subst = FxHashMap::<TyVarId, Ty>::default();
    if !match_type_shape(&current, expected_result_type, &mut subst) {
        return None;
    }

    Some(
        fields
            .into_iter()
            .map(|field| apply_type_subst(field, &subst))
            .collect(),
    )
}

fn match_type_shape(expected: &Ty, actual: &Ty, subst: &mut FxHashMap<TyVarId, Ty>) -> bool {
    match (expected, actual) {
        (Ty::TyVar(id), _) => match subst.get(id) {
            Some(existing) => existing == actual,
            None => {
                subst.insert(*id, actual.clone());
                true
            }
        },
        (Ty::TyUnit, Ty::TyUnit) => true,
        (Ty::TyCon(expected), Ty::TyCon(actual)) => expected == actual,
        (Ty::TyExVar(expected), Ty::TyExVar(actual)) => expected == actual,
        (Ty::TyArr(expected_lhs, expected_rhs), Ty::TyArr(actual_lhs, actual_rhs))
        | (Ty::TyApp(expected_lhs, expected_rhs), Ty::TyApp(actual_lhs, actual_rhs)) => {
            match_type_shape(expected_lhs, actual_lhs, subst)
                && match_type_shape(expected_rhs, actual_rhs, subst)
        }
        (Ty::TyForAll(_, _, _), _) => {
            let (_, _, body) = expected.peel_scheme_like();
            match_type_shape(&body, actual, subst)
        }
        _ => false,
    }
}

fn apply_type_subst(ty: Ty, subst: &FxHashMap<TyVarId, Ty>) -> Ty {
    subst.iter().fold(ty, |acc, (var, replacement)| {
        acc.substitute(*var, replacement)
    })
}

fn collect_textual_let_hints(
    file: FileId,
    source: &str,
    checked: &TermExprNode<TcMetadata, Range<usize>>,
    tables: PrettyTables<'_>,
    out: &mut Vec<SourceInlayHint>,
) {
    let checked = strip_checked_wrappers(checked);

    match &checked.node {
        TermExpr::Let(name, binder, body) => {
            collect_textual_let_hints(file, source, binder, tables, out);
            if let Some(position) = let_binding_hint_offset(source, &checked.anno.span, name) {
                push_let_binding_hint(file, position, binder, tables, out);
            }
            collect_textual_let_hints(file, source, body, tables, out);
        }
        TermExpr::Abs(_, _, body)
        | TermExpr::TyAbs(_, _, body)
        | TermExpr::TyApp(body, _)
        | TermExpr::EvAbs(_, _, body)
        | TermExpr::EvApp(body, _)
        | TermExpr::Anno(body, _) => collect_textual_let_hints(file, source, body, tables, out),
        TermExpr::App(lhs, rhs) => {
            collect_textual_let_hints(file, source, lhs, tables, out);
            collect_textual_let_hints(file, source, rhs, tables, out);
        }
        TermExpr::Case(scrutinee, arms) => {
            collect_textual_let_hints(file, source, scrutinee, tables, out);
            for arm in arms {
                collect_textual_let_hints(file, source, &arm.node.expr, tables, out);
            }
        }
        TermExpr::Match(scrutinees, arms) => {
            for scrutinee in scrutinees {
                collect_textual_let_hints(file, source, scrutinee, tables, out);
            }
            for arm in arms {
                collect_textual_let_hints(file, source, &arm.node.expr, tables, out);
            }
        }
        TermExpr::IfThenElse(cond, then_branch, else_branch) => {
            collect_textual_let_hints(file, source, cond, tables, out);
            collect_textual_let_hints(file, source, then_branch, tables, out);
            collect_textual_let_hints(file, source, else_branch, tables, out);
        }
        TermExpr::Unit
        | TermExpr::Var(_)
        | TermExpr::Lit(_)
        | TermExpr::Hole
        | TermExpr::Con(_) => {}
    }
}

fn push_let_binding_hint(
    file: FileId,
    position: u32,
    binder: &TermExprNode<TcMetadata, Range<usize>>,
    tables: PrettyTables<'_>,
    out: &mut Vec<SourceInlayHint>,
) {
    out.push(SourceInlayHint::new(
        FileSpan::new(file, Span::new(position, position)),
        format!(": {}", binder.anno.inferred_type.pretty_with_tables(tables)),
        SourceInlayHintKind::Type,
        SourceInlayHintCategory::LetBindingType,
    ));
}

fn push_local_hover(
    file: FileId,
    span: Span,
    name: &str,
    ty: &Ty,
    tables: PrettyTables<'_>,
    out: &mut Vec<HoverInfo>,
) {
    if span.is_empty() {
        return;
    }

    out.push(HoverInfo::new(
        file,
        span.to_usize_range(),
        format!("{name} : {}", ty.pretty_with_tables(tables)),
        None,
    ));
}

fn checked_body_without_function_params(
    body: &TermExprNode<TcMetadata, Range<usize>>,
    param_count: usize,
) -> &TermExprNode<TcMetadata, Range<usize>> {
    let mut current = strip_checked_wrappers(body);

    for _ in 0..param_count {
        let TermExpr::Abs(_, _, body) = &current.node else {
            break;
        };
        current = strip_checked_wrappers(body);
    }

    current
}

fn strip_checked_wrappers(
    expr: &TermExprNode<TcMetadata, Range<usize>>,
) -> &TermExprNode<TcMetadata, Range<usize>> {
    let mut current = expr;

    loop {
        current = match &current.node {
            TermExpr::TyAbs(_, _, body)
            | TermExpr::TyApp(body, _)
            | TermExpr::EvAbs(_, _, body)
            | TermExpr::EvApp(body, _) => body,
            _ => return current,
        };
    }
}

fn let_binding_hint_offset(source: &str, span: &Range<usize>, name: &str) -> Option<u32> {
    let source_span = source.get(span.clone())?;
    let (_, let_end) = find_word(source_span, "let", 0)?;
    let (_, end) = find_word(source_span, name, let_end)?;
    u32::try_from(span.start + end).ok()
}

fn find_word(source: &str, word: &str, start: usize) -> Option<(usize, usize)> {
    let mut search_start = start.min(source.len());

    while let Some(relative) = source.get(search_start..)?.find(word) {
        let word_start = search_start + relative;
        let word_end = word_start + word.len();

        if is_word_start_boundary(source, word_start) && is_word_end_boundary(source, word_end) {
            return Some((word_start, word_end));
        }

        search_start = word_end;
    }

    None
}

fn is_word_start_boundary(source: &str, offset: usize) -> bool {
    offset == 0
        || source.get(..offset).is_none_or(|prefix| {
            prefix
                .chars()
                .next_back()
                .is_none_or(|ch| !is_identifier_char(ch))
        })
}

fn is_word_end_boundary(source: &str, offset: usize) -> bool {
    offset >= source.len()
        || source.get(offset..).is_none_or(|suffix| {
            suffix
                .chars()
                .next()
                .is_none_or(|ch| !is_identifier_char(ch))
        })
}

fn is_identifier_char(ch: char) -> bool {
    ch == '_' || ch == '\'' || ch.is_alphanumeric()
}

fn span_contains_position(span: Span, position: u32) -> bool {
    if span.is_empty() {
        span.start == position
    } else {
        span.start <= position && position <= span.end
    }
}

fn inlay_display_tables(modules: &[CheckedModule]) -> (TyConTable, CtorTable, ClassTable) {
    let mut tycon_table = TyConTable::new();
    let mut ctor_table = CtorTable::new();
    let mut class_table = ClassTable::new();

    for module in modules {
        for (id, info) in module.tycon_table.iter() {
            tycon_table.insert_with_id(id, info.clone());
        }

        for (id, info) in module.ctor_table.iter() {
            ctor_table.insert_with_id(id, info.clone());
        }

        for (id, info) in module.class_table.iter() {
            class_table.insert_with_id(id, info.clone());
        }
    }

    (tycon_table, ctor_table, class_table)
}

fn inlay_hint_sort_key(
    hint: &SourceInlayHint,
) -> (FileId, u32, u32, SourceInlayHintCategory, String) {
    let span = hint.file_span();

    (
        span.file,
        span.span.start,
        span.span.end,
        hint.category(),
        hint.label().to_string(),
    )
}

fn local_hover_sort_key(hover: &HoverInfo) -> (FileId, usize, usize, String) {
    let span = hover.file_span();

    (
        span.file,
        hover.span().start,
        hover.span().end,
        hover.signature().to_string(),
    )
}
