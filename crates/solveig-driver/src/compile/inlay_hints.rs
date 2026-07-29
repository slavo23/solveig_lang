//! Source-level inlay hint collection and filtering.

use super::*;

pub(super) fn project_inlay_hints(
    session: &Session,
    project: &Project,
    parsed: &ParsedProject,
    order: &[ModuleName],
    checked: &FxHashMap<ModuleId, CheckedModule>,
    names: &Names,
    modules: &[CheckedModule],
) -> Vec<SourceInlayHint> {
    let (tycon_table, class_table) = inlay_display_tables(modules);
    let tables = PrettyTables {
        names,
        tycons: &tycon_table,
        classes: &class_table,
    };
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

    for defn in &parsed.defs {
        let SurfaceDefinition::FunDef {
            name, body, params, ..
        } = &defn.0
        else {
            continue;
        };
        let def_id = DefId {
            module: module_id,
            name: name.0,
        };
        let Some(checked_def) = checked_defs.get(&def_id).copied() else {
            continue;
        };
        let checked_body = collect_function_param_hints(
            session,
            parsed.file,
            params,
            &checked_def.body,
            tables,
            out,
        )
        .unwrap_or_else(|| checked_body_without_function_params(&checked_def.body, params.len()));

        if !signatures.contains(&name.0)
            && let Some(scheme) = checked.func_schemes.get(&def_id)
        {
            push_inferred_signature_hint(parsed.file, name, scheme, tables, out);
        }

        collect_expr_inlay_hints(session, parsed.file, body, checked_body, tables, out);
        collect_textual_let_hints(parsed.file, source, checked_body, tables, out);
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
        format!(": {}", scheme.pretty_with_tables(tables)),
        SourceInlayHintKind::Type,
        SourceInlayHintCategory::InferredSignature,
    ));
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
            SurfaceExpr::Let(name, binder, body),
            TermExpr::Let(checked_name, checked_binder, checked_body),
        ) if checked_name == &*session.str(name.0) => {
            push_let_binding_hint(file, name.1.end, checked_binder, tables, out);
            collect_expr_inlay_hints(session, file, binder, checked_binder, tables, out);
            collect_expr_inlay_hints(session, file, body, checked_body, tables, out);
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

fn collect_lambda_param_hints<'a>(
    session: &Session,
    file: FileId,
    params: &[(Spanned<Symbol>, Option<Spanned<SurfaceTy>>)],
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
        if checked_name != &*session.str(param.0) {
            return None;
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

        current = body;
    }

    Some(strip_checked_wrappers(current))
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

    while let Some(relative) = source[search_start..].find(word) {
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
        || source[..offset]
            .chars()
            .next_back()
            .is_none_or(|ch| !is_identifier_char(ch))
}

fn is_word_end_boundary(source: &str, offset: usize) -> bool {
    offset >= source.len()
        || source[offset..]
            .chars()
            .next()
            .is_none_or(|ch| !is_identifier_char(ch))
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

fn inlay_display_tables(modules: &[CheckedModule]) -> (TyConTable, ClassTable) {
    let mut tycon_table = TyConTable::new();
    let mut class_table = ClassTable::new();

    for module in modules {
        for (id, info) in module.tycon_table.iter() {
            tycon_table.insert_with_id(id, info.clone());
        }

        for (id, info) in module.class_table.iter() {
            class_table.insert_with_id(id, info.clone());
        }
    }

    (tycon_table, class_table)
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
