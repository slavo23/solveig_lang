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
    let tables = PrettyTables {
        names: &artifacts.names,
        tycons: &artifacts.tycon_table,
        classes: &artifacts.class_table,
    };
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
    let label = format!("{name} : {}", pretty_signature_ty(&signature_type, tables));
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
        params.push(pretty_signature_ty(&param, tables));
        current = *rest;
    }

    params
}

fn pretty_signature_ty(ty: &Ty, tables: PrettyTables<'_>) -> String {
    const PRECEDENCE_LOOSE: u8 = 0;
    const PRECEDENCE_ARR: u8 = 1;
    const PRECEDENCE_APP: u8 = 2;
    const PRECEDENCE_ATOM: u8 = 3;

    fn precedence(ty: &Ty) -> u8 {
        match ty {
            Ty::TyForAll(_, _, _) => PRECEDENCE_LOOSE,
            Ty::TyArr(_, _) => PRECEDENCE_ARR,
            Ty::TyApp(_, _) => PRECEDENCE_APP,
            Ty::TyUnit | Ty::TyVar(_) | Ty::TyExVar(_) | Ty::TyCon(_) => PRECEDENCE_ATOM,
        }
    }

    fn go(ty: &Ty, parent_precedence: u8, tables: PrettyTables<'_>) -> String {
        let current_precedence = precedence(ty);
        let text = match ty {
            Ty::TyUnit => "()".to_string(),
            Ty::TyVar(id) => tables.names.tyvar_display_name(*id),
            Ty::TyExVar(_) => ty.pretty(),
            Ty::TyCon(id) => signature_tycon_name(*id, tables),
            Ty::TyArr(lhs, rhs) => {
                let lhs = go(lhs, PRECEDENCE_APP, tables);
                let rhs = go(rhs, PRECEDENCE_ARR, tables);

                format!("{lhs} -> {rhs}")
            }
            Ty::TyForAll(_, _, _) => {
                let (kinded_quants, preds, rest) = ty.peel_scheme_like();
                let quantifiers = kinded_quants
                    .iter()
                    .map(|(id, kind)| {
                        format!(
                            "({} : {})",
                            tables.names.tyvar_display_name(*id),
                            kind.pretty()
                        )
                    })
                    .collect::<Vec<_>>();
                let prefix = if quantifiers.is_empty() {
                    String::new()
                } else {
                    format!("forall {}. ", quantifiers.join(" "))
                };

                if preds.is_empty() {
                    format!("{prefix}{}", go(&rest, PRECEDENCE_LOOSE, tables))
                } else {
                    let predicates = if preds.len() == 1 {
                        pretty_signature_predicate(&preds[0], tables)
                    } else {
                        format!(
                            "({})",
                            preds
                                .iter()
                                .map(|pred| pretty_signature_predicate(pred, tables))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    };

                    format!(
                        "{prefix}{predicates} => {}",
                        go(&rest, PRECEDENCE_LOOSE, tables)
                    )
                }
            }
            Ty::TyApp(func, arg) => {
                let func = go(func, PRECEDENCE_APP, tables);
                let arg = go(arg, PRECEDENCE_ATOM, tables);

                format!("{func} {arg}")
            }
        };

        if current_precedence < parent_precedence {
            format!("({text})")
        } else {
            text
        }
    }

    go(ty, PRECEDENCE_LOOSE, tables)
}

fn pretty_signature_predicate(pred: &Predicate, tables: PrettyTables<'_>) -> String {
    let class = tables.classes.get(pred.class_id).map_or_else(
        || tables.names.class_display_name(pred.class_id),
        |info| tables.names.symbol_display_name(info.short_name),
    );
    let args = pred
        .args
        .iter()
        .map(|arg| {
            let text = pretty_signature_ty(arg, tables);
            match arg {
                Ty::TyArr(_, _) | Ty::TyForAll(_, _, _) => format!("({text})"),
                _ => text,
            }
        })
        .collect::<Vec<_>>();

    if args.is_empty() {
        class
    } else {
        format!("{class} {}", args.join(" "))
    }
}

fn signature_tycon_name(id: TyConId, tables: PrettyTables<'_>) -> String {
    tables.tycons.get(id).map_or_else(
        || tables.names.tycon_display_name(id),
        |info| tables.names.symbol_display_name(info.short_name),
    )
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
