//! Inlay and hover facts for grouped explicit instance methods.
//! The checked match arms are paired with their originating surface equations.

use super::*;

pub(super) fn collect_instance_method_group_inlay_hints(
    cx: InlayGroupContext<'_>,
    methods: &[InstanceMethod],
    group: &[usize],
    checked_method: &InstanceMethodDef<TcMetadata, Range<usize>>,
    out: &mut Vec<SourceInlayHint>,
) {
    let Some(first_method) = group.first().and_then(|index| methods.get(*index)) else {
        return;
    };
    let checked_body =
        checked_body_without_function_params(&checked_method.body, first_method.params.len());

    if group.len() == 1
        && let Some(simple_params) = simple_variable_params(&first_method.params)
        && let Some(checked_body) = collect_function_param_hints(
            cx.session,
            cx.parsed.file,
            &simple_params,
            &checked_method.body,
            cx.tables,
            out,
        )
    {
        collect_expr_inlay_hints(
            cx.session,
            cx.parsed.file,
            &first_method.body,
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
    for (method_index, checked_arm) in group.iter().zip(arms) {
        let Some(method) = methods.get(*method_index) else {
            continue;
        };
        collect_expr_inlay_hints(
            cx.session,
            cx.parsed.file,
            &method.body,
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

pub(super) fn collect_instance_method_group_local_hovers(
    cx: LocalHoverContext<'_>,
    methods: &[InstanceMethod],
    group: &[usize],
    checked_method: &InstanceMethodDef<TcMetadata, Range<usize>>,
    out: &mut Vec<HoverInfo>,
) {
    let Some(first_method) = group.first().and_then(|index| methods.get(*index)) else {
        return;
    };
    for method_index in group {
        let Some(method) = methods.get(*method_index) else {
            continue;
        };
        push_local_hover(
            cx.file,
            method.name.1,
            &checked_method.name,
            &checked_method.body.anno.inferred_type,
            cx.tables,
            out,
        );
    }

    let checked_body =
        checked_body_without_function_params(&checked_method.body, first_method.params.len());
    if group.len() == 1 {
        let mut param_facts = Vec::new();
        if let Some(checked_body) =
            simple_variable_params(&first_method.params).and_then(|simple_params| {
                collect_function_param_hovers(
                    cx.session,
                    cx.file,
                    &simple_params,
                    &checked_method.body,
                    cx.tables,
                    &mut param_facts,
                    out,
                )
            })
        {
            for (symbol, name, ty) in &param_facts {
                push_expr_reference_hovers(
                    cx.file,
                    &first_method.body,
                    *symbol,
                    name,
                    ty,
                    cx.tables,
                    out,
                );
            }
            collect_expr_local_hovers(cx, &first_method.body, checked_body, out);
            return;
        }
    }

    let TermExpr::Match(scrutinees, arms) = &checked_body.node else {
        return;
    };
    for (method_index, checked_arm) in group.iter().zip(arms) {
        let Some(method) = methods.get(*method_index) else {
            continue;
        };
        let mut pattern_facts = Vec::new();
        {
            let mut sink = PatternLocalHoverSink {
                binder_facts: &mut pattern_facts,
                out,
            };
            for ((surface_param, checked_pattern), checked_scrutinee) in method
                .params
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
            push_expr_reference_hovers(cx.file, &method.body, *symbol, name, ty, cx.tables, out);
        }
        collect_expr_local_hovers(cx, &method.body, &checked_arm.node.expr, out);
    }
}
