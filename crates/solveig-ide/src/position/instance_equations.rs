//! Position-index expression facts for grouped explicit instance methods.
//! Each checked match arm is associated with its original surface body.

use super::*;

pub(super) fn collect_instance_method_group_position_facts(
    methods: &[InstanceMethod],
    group: &[usize],
    checked_method: &InstanceMethodDef<TcMetadata, Range<usize>>,
    cx: PositionFactContext<'_>,
    index: &mut PositionIndex,
) {
    let Some(first_method) = group
        .first()
        .and_then(|method_index| methods.get(*method_index))
    else {
        return;
    };
    let checked_body =
        checked_body_without_function_params(&checked_method.body, first_method.params.len());

    if group.len() == 1 && !matches!(checked_body.node, TermExpr::Match(_, _)) {
        collect_expr_position_facts(
            &first_method.body,
            checked_body,
            cx.snapshot,
            cx.docs,
            cx.ctor_table,
            cx.tables,
            index,
        );
        return;
    }

    let TermExpr::Match(_, arms) = &checked_body.node else {
        return;
    };
    for (method_index, checked_arm) in group.iter().zip(arms) {
        let Some(method) = methods.get(*method_index) else {
            continue;
        };
        collect_expr_position_facts(
            &method.body,
            &checked_arm.node.expr,
            cx.snapshot,
            cx.docs,
            cx.ctor_table,
            cx.tables,
            index,
        );
    }
}
