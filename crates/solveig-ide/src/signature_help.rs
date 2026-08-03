//! Source-level signature help over retained checked expressions.

use super::source_lookup::{source_file_id_in_source_map, span_len};
use super::*;

#[derive(Clone, Debug)]
struct SignatureCandidate {
    span: Range<usize>,
    help: SourceSignatureHelp,
}

type CheckedExpr = TermExprNode<TcMetadata, Range<usize>>;

pub(super) fn signature_help_at_position_in_artifacts(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
) -> Option<SourceSignatureHelp> {
    let file_id = source_file_id_in_source_map(source_map, file)?;
    let module_id = artifacts
        .module_files
        .iter()
        .find_map(|(module_id, module_file)| (*module_file == file_id).then_some(*module_id))?;
    let checked = artifacts.checked.get(&module_id)?;
    let offset = source_map.offset(file_id, position, encoding) as usize;
    let tables = PrettyTables::new(
        &artifacts.names,
        &artifacts.tycon_table,
        &artifacts.class_table,
    );
    let mut best = None;

    for def in &checked.func_defs {
        collect_signature_help(artifacts, &def.body, tables, offset, &mut best);
    }
    for instance in &checked.instance_defs {
        for method in &instance.methods {
            collect_signature_help(artifacts, &method.body, tables, offset, &mut best);
        }
    }

    best.map(|candidate: SignatureCandidate| candidate.help)
}

fn collect_signature_help(
    artifacts: &CompilationArtifacts,
    expr: &CheckedExpr,
    tables: PrettyTables<'_>,
    offset: usize,
    best: &mut Option<SignatureCandidate>,
) {
    if matches!(strip_wrappers(expr).node, TermExpr::App(_, _))
        && let Some((help, span)) = signature_help_for_application(artifacts, expr, tables, offset)
    {
        consider_signature_candidate(best, SignatureCandidate { span, help });
    }

    match &strip_wrappers(expr).node {
        TermExpr::App(func, arg) => {
            collect_signature_help(artifacts, func, tables, offset, best);
            collect_signature_help(artifacts, arg, tables, offset, best);
        }
        TermExpr::Abs(_, _, body)
        | TermExpr::TyAbs(_, _, body)
        | TermExpr::TyApp(body, _)
        | TermExpr::EvAbs(_, _, body)
        | TermExpr::EvApp(body, _)
        | TermExpr::Anno(body, _) => collect_signature_help(artifacts, body, tables, offset, best),
        TermExpr::Case(scrutinee, arms) => {
            collect_signature_help(artifacts, scrutinee, tables, offset, best);
            for arm in arms {
                collect_signature_help(artifacts, &arm.node.expr, tables, offset, best);
            }
        }
        TermExpr::IfThenElse(cond, then_branch, else_branch) => {
            collect_signature_help(artifacts, cond, tables, offset, best);
            collect_signature_help(artifacts, then_branch, tables, offset, best);
            collect_signature_help(artifacts, else_branch, tables, offset, best);
        }
        TermExpr::Let(_, binder, body) => {
            collect_signature_help(artifacts, binder, tables, offset, best);
            collect_signature_help(artifacts, body, tables, offset, best);
        }
        TermExpr::Unit
        | TermExpr::Var(_)
        | TermExpr::Lit(_)
        | TermExpr::Hole
        | TermExpr::Con(_) => {}
    }
}

fn signature_help_for_application(
    artifacts: &CompilationArtifacts,
    expr: &CheckedExpr,
    tables: PrettyTables<'_>,
    offset: usize,
) -> Option<(SourceSignatureHelp, Range<usize>)> {
    let (head, args) = flatten_application(expr);
    if args.is_empty() {
        return None;
    }
    let span = application_span(expr, &args);
    if !span_contains_offset_inclusive(&span, offset) {
        return None;
    }

    let name = callable_name(artifacts, head)?;
    let signature_type = callable_type(artifacts, head);
    let parameters = arrow_parameter_types(&signature_type, tables);
    if parameters.is_empty() {
        return None;
    }

    let active_parameter = active_parameter(&args, offset);
    let label = format!(
        "{name} : {}",
        source_signature::pretty_source_ty(&signature_type, tables)
    );
    let signature = SourceSignatureInformation::new(label, parameters)
        .with_optional_documentation(callable_documentation(artifacts, head));

    Some((
        SourceSignatureHelp::new(signature, 0, active_parameter),
        span,
    ))
}

fn flatten_application(expr: &CheckedExpr) -> (&CheckedExpr, Vec<&CheckedExpr>) {
    let mut current = expr;
    let mut args = Vec::new();

    while let TermExpr::App(func, arg) = &strip_wrappers(current).node {
        args.push(&**arg);
        current = func;
    }

    args.reverse();
    (current, args)
}

fn application_span(expr: &CheckedExpr, args: &[&CheckedExpr]) -> Range<usize> {
    let start = expr.anno.span.start;
    let end = args
        .last()
        .map_or(expr.anno.span.end, |arg| arg.anno.span.end)
        .max(expr.anno.span.end);

    start..end
}

fn span_contains_offset_inclusive(span: &Range<usize>, offset: usize) -> bool {
    if span.start == span.end {
        offset == span.start
    } else {
        span.start <= offset && offset <= span.end
    }
}

fn callable_name(artifacts: &CompilationArtifacts, head: &CheckedExpr) -> Option<String> {
    match &strip_wrappers(head).node {
        TermExpr::Var(var) => Some(var.display_name(&artifacts.names)),
        TermExpr::Con(id) => Some(artifacts.names.ctor_display_name(*id)),
        _ => None,
    }
}

fn callable_type(artifacts: &CompilationArtifacts, head: &CheckedExpr) -> Ty {
    match &strip_wrappers(head).node {
        TermExpr::Var(VarRef::Global(def_id)) => global_callable_type(artifacts, *def_id)
            .unwrap_or_else(|| head.anno.inferred_type.clone()),
        TermExpr::Con(id) => artifacts.ctor_table.get(*id).map_or_else(
            || head.anno.inferred_type.clone(),
            |info| info.full_type.clone(),
        ),
        _ => head.anno.inferred_type.clone(),
    }
}

fn global_callable_type(artifacts: &CompilationArtifacts, def_id: DefId) -> Option<Ty> {
    for checked in artifacts.checked.values() {
        if let Some(scheme) = checked.func_schemes.get(&def_id) {
            return Some(scheme.to_ty());
        }

        for class_def in &checked.class_defs {
            if let Some(method) = class_def.methods.iter().find(|method| method.id == def_id) {
                return Some(method.scheme.to_ty());
            }
        }
    }

    None
}

fn callable_documentation(artifacts: &CompilationArtifacts, head: &CheckedExpr) -> Option<String> {
    match &strip_wrappers(head).node {
        TermExpr::Var(VarRef::Global(def_id)) => {
            artifacts.docs.get(def_id).map(|doc| doc.text.clone())
        }
        _ => None,
    }
}

fn arrow_parameter_types(ty: &Ty, tables: PrettyTables<'_>) -> Vec<String> {
    let (_, _, mut current) = ty.peel_scheme_like();
    let mut params = Vec::new();

    while let Ty::TyArr(param, rest) = current {
        params.push(source_signature::pretty_source_ty(&param, tables));
        current = *rest;
    }

    params
}

fn active_parameter(args: &[&CheckedExpr], offset: usize) -> u32 {
    let mut active = 0usize;

    for (index, arg) in args.iter().enumerate() {
        active = index;
        if offset <= arg.anno.span.end {
            break;
        }
    }

    u32::try_from(active.min(args.len().saturating_sub(1))).unwrap_or(u32::MAX)
}

fn strip_wrappers(expr: &CheckedExpr) -> &CheckedExpr {
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

fn consider_signature_candidate(
    best: &mut Option<SignatureCandidate>,
    candidate: SignatureCandidate,
) {
    let replace = best.as_ref().is_none_or(|current| {
        span_len(&candidate.span) < span_len(&current.span)
            || (span_len(&candidate.span) == span_len(&current.span))
    });

    if replace {
        *best = Some(candidate);
    }
}
