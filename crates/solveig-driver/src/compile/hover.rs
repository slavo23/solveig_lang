//! Hover lookup over retained checked artifacts.
//! The driver keeps enough table state here to render types after checking.

use super::*;

#[derive(Clone, Debug)]
struct HoverCandidate {
    span: Range<usize>,
    ty: String,
}

pub(super) fn hover_in_artifacts(
    artifacts: &CompilationArtifacts,
    file: &Path,
    offset: usize,
) -> Option<String> {
    let file_id = source_file_id(artifacts, file)?;
    let pretty_tables = PrettyTables {
        names: &artifacts.names,
        tycons: &artifacts.tycon_table,
        classes: &artifacts.class_table,
    };
    let mut best = None;

    for (module_id, checked) in &artifacts.checked {
        if artifacts.module_files.get(module_id) != Some(&file_id) {
            continue;
        }

        for func_def in &checked.func_defs {
            hover_expr(&func_def.body, pretty_tables, offset, &mut best);
        }

        for instance in &checked.instance_defs {
            for method in &instance.methods {
                hover_expr(&method.body, pretty_tables, offset, &mut best);
            }
        }
    }

    best.map(|candidate| candidate.ty)
}

fn source_file_id(artifacts: &CompilationArtifacts, file: &Path) -> Option<FileId> {
    artifacts
        .source_files
        .iter()
        .enumerate()
        .find(|(_, source)| source.path == file)
        .or_else(|| {
            artifacts
                .source_files
                .iter()
                .enumerate()
                .find(|(_, source)| file.is_relative() && source.path.ends_with(file))
        })
        .map(|(index, _)| FileId(index as u32))
}

fn hover_expr(
    expr: &TermExprNode<TcMetadata, Range<usize>>,
    pretty_tables: PrettyTables<'_>,
    offset: usize,
    best: &mut Option<HoverCandidate>,
) {
    if !span_contains_offset(&expr.anno.span, offset) {
        return;
    }

    consider_hover_candidate(
        best,
        expr.anno.span.clone(),
        expr.anno.inferred_type.pretty_with_tables(pretty_tables),
    );

    match &expr.node {
        TermExpr::Unit
        | TermExpr::Var(_)
        | TermExpr::Lit(_)
        | TermExpr::Hole
        | TermExpr::Con(_) => {}
        TermExpr::App(func, arg) => {
            hover_expr(func, pretty_tables, offset, best);
            hover_expr(arg, pretty_tables, offset, best);
        }
        TermExpr::Abs(_, _, body)
        | TermExpr::TyAbs(_, _, body)
        | TermExpr::TyApp(body, _)
        | TermExpr::EvAbs(_, _, body)
        | TermExpr::EvApp(body, _)
        | TermExpr::Anno(body, _) => hover_expr(body, pretty_tables, offset, best),
        TermExpr::Case(scrutinee, arms) => {
            hover_expr(scrutinee, pretty_tables, offset, best);
            for arm in arms {
                hover_expr(&arm.node.expr, pretty_tables, offset, best);
            }
        }
        TermExpr::IfThenElse(cond, then_branch, else_branch) => {
            hover_expr(cond, pretty_tables, offset, best);
            hover_expr(then_branch, pretty_tables, offset, best);
            hover_expr(else_branch, pretty_tables, offset, best);
        }
        TermExpr::Let(_, binder, in_expr) => {
            hover_expr(binder, pretty_tables, offset, best);
            hover_expr(in_expr, pretty_tables, offset, best);
        }
    }
}

fn span_contains_offset(span: &Range<usize>, offset: usize) -> bool {
    if span.start == span.end {
        offset == span.start
    } else {
        span.start <= offset && offset < span.end
    }
}

fn consider_hover_candidate(best: &mut Option<HoverCandidate>, span: Range<usize>, ty: String) {
    let replace = best
        .as_ref()
        .is_none_or(|current| span_len(&span) <= span_len(&current.span));

    if replace {
        *best = Some(HoverCandidate { span, ty });
    }
}

fn span_len(span: &Range<usize>) -> usize {
    span.end.saturating_sub(span.start)
}
