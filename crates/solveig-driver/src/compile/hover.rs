//! Hover lookup over retained checked artifacts.
//! The driver keeps enough table state here to render types after checking.

use super::source_lookup::{source_file_id, span_contains_offset, span_len};
use super::*;

#[derive(Clone, Debug)]
struct HoverCandidate {
    priority: u8,
    info: HoverInfo,
}

pub(super) fn hover_in_artifacts(
    artifacts: &CompilationArtifacts,
    file: &Path,
    offset: usize,
) -> Option<HoverInfo> {
    let file_id = source_file_id(artifacts, file)?;
    hover_in_file_id(artifacts, file_id, offset)
}

pub(super) fn hover_at_position_in_artifacts(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
) -> Option<HoverInfo> {
    let file_id = source_file_id(artifacts, file)?;
    let offset = source_map.offset(file_id, position, encoding) as usize;

    hover_in_file_id(artifacts, file_id, offset)
}

fn hover_in_file_id(
    artifacts: &CompilationArtifacts,
    file_id: FileId,
    offset: usize,
) -> Option<HoverInfo> {
    let pretty_tables = PrettyTables {
        names: &artifacts.names,
        tycons: &artifacts.tycon_table,
        classes: &artifacts.class_table,
    };
    let mut best = None;

    for (span, module_id) in &artifacts.module_imports {
        if span.file != file_id || !span_contains_offset(&span.span.to_usize_range(), offset) {
            continue;
        }

        if let Some(info) = module_hover_info(artifacts, *span, *module_id) {
            consider_hover_candidate(&mut best, 2, info);
        }
    }

    for (span, def_id) in &artifacts.declaration_spans {
        if span.file != file_id || !span_contains_offset(&span.span.to_usize_range(), offset) {
            continue;
        }

        if let Some(info) = def_hover_info(artifacts, pretty_tables, *span, *def_id) {
            consider_hover_candidate(&mut best, 3, info);
        }
    }

    for resolutions in artifacts.resolutions.values() {
        for (span, def_id) in resolutions {
            if span.file != file_id || !span_contains_offset(&span.span.to_usize_range(), offset) {
                continue;
            }

            if let Some(info) = def_hover_info(artifacts, pretty_tables, *span, *def_id) {
                consider_hover_candidate(&mut best, 3, info);
            }
        }
    }

    for (module_id, checked) in &artifacts.checked {
        if artifacts.module_files.get(module_id) != Some(&file_id) {
            continue;
        }

        for func_def in &checked.func_defs {
            hover_expr(file_id, &func_def.body, pretty_tables, offset, &mut best);
        }

        for instance in &checked.instance_defs {
            for method in &instance.methods {
                hover_expr(file_id, &method.body, pretty_tables, offset, &mut best);
            }
        }
    }

    best.map(|candidate| candidate.info)
}

fn module_hover_info(
    artifacts: &CompilationArtifacts,
    span: FileSpan,
    module_id: ModuleId,
) -> Option<HoverInfo> {
    let name = artifacts.module_display_names.get(&module_id)?;
    let documentation = artifacts
        .module_paths
        .get(&module_id)
        .map(|path| path.display().to_string());

    Some(HoverInfo::new(
        span.file,
        span.span.to_usize_range(),
        format!("module {name}"),
        documentation,
    ))
}

fn def_hover_info(
    artifacts: &CompilationArtifacts,
    pretty_tables: PrettyTables<'_>,
    span: FileSpan,
    def_id: DefId,
) -> Option<HoverInfo> {
    let signature = def_signature(artifacts, pretty_tables, def_id)?;
    let documentation = artifacts.docs.get(&def_id).map(|doc| doc.text.clone());

    Some(HoverInfo::new(
        span.file,
        span.span.to_usize_range(),
        signature,
        documentation,
    ))
}

fn def_signature(
    artifacts: &CompilationArtifacts,
    pretty_tables: PrettyTables<'_>,
    def_id: DefId,
) -> Option<String> {
    class_method_signature(artifacts, pretty_tables, def_id)
        .or_else(|| term_signature(artifacts, pretty_tables, def_id))
        .or_else(|| constructor_signature(artifacts, pretty_tables, def_id))
        .or_else(|| tycon_signature(artifacts, def_id))
        .or_else(|| class_signature(artifacts, pretty_tables, def_id))
}

fn term_signature(
    artifacts: &CompilationArtifacts,
    pretty_tables: PrettyTables<'_>,
    def_id: DefId,
) -> Option<String> {
    let scheme = artifacts
        .checked
        .values()
        .find_map(|checked| checked.func_schemes.get(&def_id))?;

    Some(format!(
        "{} : {}",
        short_def_name(&artifacts.names, def_id),
        scheme.pretty_with_tables(pretty_tables)
    ))
}

fn constructor_signature(
    artifacts: &CompilationArtifacts,
    pretty_tables: PrettyTables<'_>,
    def_id: DefId,
) -> Option<String> {
    let full_name = artifacts.names.def_display_name(def_id);
    let info = artifacts.ctor_table.iter().find_map(|(_, info)| {
        (artifacts.names.symbol_display_name(info.name) == full_name).then_some(info)
    })?;

    Some(format!(
        "{} : {}",
        artifacts.names.symbol_display_name(info.short_name),
        info.full_type.pretty_with_tables(pretty_tables)
    ))
}

fn tycon_signature(artifacts: &CompilationArtifacts, def_id: DefId) -> Option<String> {
    let full_name = artifacts.names.def_display_name(def_id);
    let info = artifacts.tycon_table.iter().find_map(|(_, info)| {
        (artifacts.names.symbol_display_name(info.name) == full_name).then_some(info)
    })?;

    Some(format!(
        "type {} : {}",
        artifacts.names.symbol_display_name(info.short_name),
        info.kind.pretty()
    ))
}

fn class_signature(
    artifacts: &CompilationArtifacts,
    pretty_tables: PrettyTables<'_>,
    def_id: DefId,
) -> Option<String> {
    let full_name = artifacts.names.def_display_name(def_id);
    let info = artifacts.class_table.iter().find_map(|(_, info)| {
        (artifacts.names.symbol_display_name(info.name) == full_name).then_some(info)
    })?;

    Some(format!(
        "class {}",
        class_header(&info.definition, pretty_tables)
    ))
}

fn class_method_signature(
    artifacts: &CompilationArtifacts,
    pretty_tables: PrettyTables<'_>,
    def_id: DefId,
) -> Option<String> {
    for checked in artifacts.checked.values() {
        for class_def in &checked.class_defs {
            if let Some(method) = class_def.methods.iter().find(|method| method.id == def_id) {
                return Some(format!(
                    "class {}\n{} : {}",
                    class_header(class_def, pretty_tables),
                    method.short_name,
                    method.scheme.pretty_with_tables(pretty_tables)
                ));
            }
        }
    }

    None
}

fn class_header(class_def: &ClassDefinition, pretty_tables: PrettyTables<'_>) -> String {
    let mut head = class_def.short_name.clone();
    for (param, _) in &class_def.params {
        head.push(' ');
        head.push_str(&pretty_tables.names.tyvar_display_name(*param));
    }

    if class_def.superclasses.is_empty() {
        head
    } else {
        let superclasses = class_def
            .superclasses
            .iter()
            .map(|pred| pred.pretty_with_tables(pretty_tables))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{superclasses} => {head}")
    }
}

fn short_def_name(names: &Names, def_id: DefId) -> String {
    let display = names.def_display_name(def_id);

    display
        .rsplit('.')
        .next()
        .map_or(display.clone(), |name| name.to_string())
}

fn hover_expr(
    file_id: FileId,
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
        1,
        HoverInfo::new(
            file_id,
            expr.anno.span.clone(),
            expr.anno.inferred_type.pretty_with_tables(pretty_tables),
            None,
        ),
    );

    match &expr.node {
        TermExpr::Unit
        | TermExpr::Var(_)
        | TermExpr::Lit(_)
        | TermExpr::Hole
        | TermExpr::Con(_) => {}
        TermExpr::App(func, arg) => {
            hover_expr(file_id, func, pretty_tables, offset, best);
            hover_expr(file_id, arg, pretty_tables, offset, best);
        }
        TermExpr::Abs(_, _, body)
        | TermExpr::TyAbs(_, _, body)
        | TermExpr::TyApp(body, _)
        | TermExpr::EvAbs(_, _, body)
        | TermExpr::EvApp(body, _)
        | TermExpr::Anno(body, _) => hover_expr(file_id, body, pretty_tables, offset, best),
        TermExpr::Case(scrutinee, arms) => {
            hover_expr(file_id, scrutinee, pretty_tables, offset, best);
            for arm in arms {
                hover_expr(file_id, &arm.node.expr, pretty_tables, offset, best);
            }
        }
        TermExpr::IfThenElse(cond, then_branch, else_branch) => {
            hover_expr(file_id, cond, pretty_tables, offset, best);
            hover_expr(file_id, then_branch, pretty_tables, offset, best);
            hover_expr(file_id, else_branch, pretty_tables, offset, best);
        }
        TermExpr::Let(_, binder, in_expr) => {
            hover_expr(file_id, binder, pretty_tables, offset, best);
            hover_expr(file_id, in_expr, pretty_tables, offset, best);
        }
    }
}

fn consider_hover_candidate(best: &mut Option<HoverCandidate>, priority: u8, info: HoverInfo) {
    let replace = best.as_ref().is_none_or(|current| {
        let current_len = span_len(&current.info.span());
        let candidate_len = span_len(&info.span());

        candidate_len < current_len
            || (candidate_len == current_len && priority >= current.priority)
    });

    if replace {
        *best = Some(HoverCandidate { priority, info });
    }
}
