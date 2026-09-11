//! Typed completion candidates from checked module APIs and retained term bodies.
//! Surface API facts keep labels available while mid-edit code cannot be checked.

use super::*;

#[derive(Clone, Copy)]
pub(crate) struct TypedModuleCompletionInput<'a> {
    pub(crate) session: &'a Session,
    pub(crate) names: &'a Names,
    pub(crate) module_name: &'a ModuleName,
    pub(crate) parsed: &'a ParsedModule,
    pub(crate) imports: &'a [EffectiveImport],
    pub(crate) local_api: &'a ModuleApi,
    pub(crate) local_typed_api: &'a TypedModuleApi,
    pub(crate) module_apis: &'a FxHashMap<ModuleName, SharedModuleApi>,
    pub(crate) typed_apis: &'a FxHashMap<ModuleName, SharedTypedModuleApi>,
}

pub(crate) fn typed_module_completion_candidates(
    input: TypedModuleCompletionInput<'_>,
) -> Vec<SourceCompletionCandidate> {
    let TypedModuleCompletionInput {
        session,
        names,
        module_name,
        parsed,
        imports,
        local_api,
        local_typed_api,
        module_apis,
        typed_apis,
    } = input;
    let mut out = Vec::new();
    let (tycon_table, class_table) =
        completion_display_tables(session, names, Some(local_typed_api), typed_apis);
    let tables = PrettyTables::new(names, &tycon_table, &class_table);

    let local_api = local_api.local_view();
    let local_typed_api = local_typed_api.selected_by_api(&local_api);
    out.extend(typed_api_candidates(
        session,
        &local_api,
        &local_typed_api,
        parsed.file,
        None,
        tables,
    ));

    for import in imports {
        let Some(typed_api) = typed_apis.get(&import.module) else {
            continue;
        };
        let Some(source_module_api) = module_apis.get(&import.module) else {
            continue;
        };
        let module_api = source_module_api.visible_to(local_api.package);
        let typed_api = typed_api.visible_to(source_module_api, local_api.package);
        let Ok(unqualified_api) = project_module_selection(
            &module_api,
            import.span.file,
            &import.module,
            &import.selection,
        ) else {
            continue;
        };
        let unqualified_typed = typed_api.selected_by_api(&unqualified_api);

        out.extend(typed_api_candidates(
            session,
            &unqualified_api,
            &unqualified_typed,
            parsed.file,
            None,
            tables,
        ));

        for qualifier in import_qualifiers(session, module_name, import) {
            out.extend(typed_api_candidates(
                session,
                &module_api,
                &typed_api,
                parsed.file,
                Some(&qualifier),
                tables,
            ));
        }
    }

    out
}

pub(crate) fn typed_local_completion_candidates(
    checked: &CheckedModule,
    file: FileId,
    tables: PrettyTables<'_>,
) -> Vec<SourceCompletionCandidate> {
    let mut out = Vec::new();

    for def in &checked.func_defs {
        collect_typed_expr_locals(file, &def.body, tables, &mut out);
    }

    for inst in &checked.instance_defs {
        for method in &inst.methods {
            collect_typed_expr_locals(file, &method.body, tables, &mut out);
        }
    }

    out
}

fn typed_api_candidates(
    session: &Session,
    module_api: &ModuleApi,
    typed_api: &TypedModuleApi,
    file: FileId,
    qualifier: Option<&str>,
    tables: PrettyTables<'_>,
) -> Vec<SourceCompletionCandidate> {
    api_completion_items(session, module_api, Some(typed_api), tables)
        .into_iter()
        .map(|item| scoped_completion_candidate(item, file, qualifier))
        .collect()
}

pub(crate) fn module_entity_completion_items(
    session: &Session,
    names: &Names,
    module_api: &ModuleApi,
    typed_api: Option<&TypedModuleApi>,
    typed_apis: &FxHashMap<ModuleName, SharedTypedModuleApi>,
) -> Vec<SourceCompletionItem> {
    let (tycon_table, class_table) =
        completion_display_tables(session, names, typed_api, typed_apis);
    api_completion_items(
        session,
        module_api,
        typed_api,
        PrettyTables::new(names, &tycon_table, &class_table),
    )
    .into_iter()
    .filter(|item| {
        !matches!(
            item.kind(),
            SourceCompletionItemKind::Constructor | SourceCompletionItemKind::Method
        )
    })
    .collect()
}

fn api_completion_items(
    session: &Session,
    module_api: &ModuleApi,
    typed_api: Option<&TypedModuleApi>,
    tables: PrettyTables<'_>,
) -> Vec<SourceCompletionItem> {
    let mut out = Vec::new();

    for (name, def_id) in &module_api.terms {
        let detail = typed_api
            .and_then(|api| api.term_schemes.get(def_id))
            .and_then(|scheme| pretty_scheme_detail(scheme, tables));
        out.push(
            SourceCompletionItem::new(
                session.str(*name).to_string(),
                term_completion_kind(module_api, *name),
            )
            .with_optional_detail(detail)
            .with_optional_documentation(doc_for_def(module_api, typed_api, *def_id)),
        );
    }

    for (name, def_id) in &module_api.constructors {
        let detail = typed_api
            .and_then(|api| api.constructor_types.get(def_id))
            .and_then(|ty| pretty_ty_detail(ty, tables));
        out.push(
            SourceCompletionItem::new(
                session.str(*name).to_string(),
                SourceCompletionItemKind::Constructor,
            )
            .with_optional_detail(detail)
            .with_optional_documentation(doc_for_def(module_api, typed_api, *def_id)),
        );
    }

    for (name, def_id) in &module_api.types {
        out.push(
            SourceCompletionItem::new(
                session.str(*name).to_string(),
                SourceCompletionItemKind::Type,
            )
            .with_optional_detail(
                typed_api
                    .and_then(|api| api.type_kinds.get(def_id))
                    .map(|kind| format!("kind {}", kind.pretty())),
            )
            .with_optional_documentation(doc_for_def(module_api, typed_api, *def_id)),
        );
    }

    for (name, def_id) in &module_api.type_aliases {
        out.push(
            SourceCompletionItem::new(
                session.str(*name).to_string(),
                SourceCompletionItemKind::Type,
            )
            .with_detail("type alias")
            .with_optional_documentation(doc_for_def(module_api, typed_api, *def_id)),
        );
    }

    for (name, def_id) in &module_api.classes {
        let detail = typed_api
            .and_then(|api| api.class_defs.get(def_id))
            .map(|class_def| class_detail(tables.names, class_def));
        out.push(
            SourceCompletionItem::new(
                session.str(*name).to_string(),
                SourceCompletionItemKind::Class,
            )
            .with_optional_detail(detail)
            .with_optional_documentation(doc_for_def(module_api, typed_api, *def_id)),
        );
    }

    out
}

fn completion_display_tables(
    session: &Session,
    names: &Names,
    local_typed_api: Option<&TypedModuleApi>,
    typed_apis: &FxHashMap<ModuleName, SharedTypedModuleApi>,
) -> (TyConTable, ClassTable) {
    let mut tycon_table = TyConTable::new();
    let mut class_table = ClassTable::new();

    seed_completion_primitive_tycons(session, &mut tycon_table);
    if let Some(local_typed_api) = local_typed_api {
        extend_completion_display_tables(
            session,
            names,
            &mut tycon_table,
            &mut class_table,
            local_typed_api,
        );
    }
    for typed_api in typed_apis.values() {
        extend_completion_display_tables(
            session,
            names,
            &mut tycon_table,
            &mut class_table,
            typed_api,
        );
    }

    (tycon_table, class_table)
}

fn seed_completion_primitive_tycons(session: &Session, tycon_table: &mut TyConTable) {
    let primitives = [
        (PRIM_TYS.int, "prelude.builtins.Int", "Int", Kind::Star),
        (
            PRIM_TYS.float,
            "prelude.builtins.Float",
            "Float",
            Kind::Star,
        ),
        (PRIM_TYS.char, "prelude.builtins.Char", "Char", Kind::Star),
        (
            PRIM_TYS.string,
            "prelude.builtins.String",
            "String",
            Kind::Star,
        ),
        (
            PRIM_TYS.io,
            "prelude.builtins.IO",
            "IO",
            Kind::make_arrow(Kind::Star, Kind::make_arrow(Kind::Star, Kind::Star)),
        ),
    ];

    for (id, name, short_name, kind) in primitives {
        tycon_table.insert_with_id(
            id,
            TyConInfo {
                name: session.intern(name),
                short_name: session.intern(short_name),
                kind,
                definition: TyConDefinition::Primitive,
            },
        );
    }

    let bool_def = compiler_bool_data_definition();
    tycon_table.insert_with_id(
        PRIM_TYS.bool,
        TyConInfo {
            name: session.intern(&bool_def.name),
            short_name: session.intern(&bool_def.short_name),
            kind: bool_def.tycon_kind.clone(),
            definition: TyConDefinition::Data(bool_def),
        },
    );
}

fn extend_completion_display_tables(
    session: &Session,
    names: &Names,
    tycon_table: &mut TyConTable,
    class_table: &mut ClassTable,
    typed_api: &TypedModuleApi,
) {
    for (id, info) in typed_api.tycon_table.iter() {
        tycon_table.insert_with_id(id, info.clone());
    }

    for (id, info) in typed_api.class_table.iter() {
        class_table.insert_with_id(id, info.clone());
    }

    for (def_id, id) in &typed_api.type_ids {
        if tycon_table.get(*id).is_some() {
            continue;
        }

        let short_name = names.symbol_display_name(def_id.name);
        tycon_table.insert_with_id(
            *id,
            TyConInfo {
                name: session.intern(&names.tycon_display_name(*id)),
                short_name: session.intern(&short_name),
                kind: typed_api
                    .type_kinds
                    .get(def_id)
                    .cloned()
                    .unwrap_or(Kind::Star),
                definition: TyConDefinition::Primitive,
            },
        );
    }

    for class_def in typed_api.class_defs.values() {
        if class_table.get(class_def.id).is_some() {
            continue;
        }

        class_table.insert_with_id(
            class_def.id,
            ClassInfo {
                name: session.intern(&class_def.name),
                short_name: session.intern(&class_def.short_name),
                definition: class_def.clone(),
            },
        );
    }
}

fn scoped_completion_candidate(
    item: SourceCompletionItem,
    file: FileId,
    qualifier: Option<&str>,
) -> SourceCompletionCandidate {
    let candidate = SourceCompletionCandidate::new(item).in_file(file);
    match qualifier {
        Some(qualifier) => candidate.qualified(qualifier),
        None => candidate,
    }
}

fn term_completion_kind(module_api: &ModuleApi, name: Symbol) -> SourceCompletionItemKind {
    if module_api
        .class_methods
        .values()
        .any(|methods| methods.contains(&name))
    {
        SourceCompletionItemKind::Method
    } else {
        SourceCompletionItemKind::Function
    }
}

fn doc_for_def(
    module_api: &ModuleApi,
    typed_api: Option<&TypedModuleApi>,
    def_id: DefId,
) -> Option<String> {
    typed_api
        .and_then(|api| api.docs.get(&def_id))
        .or_else(|| module_api.docs.get(&def_id))
        .map(|doc| doc.text.clone())
}

fn class_detail(names: &Names, class_def: &ClassDefinition) -> String {
    let params = class_def
        .params
        .iter()
        .map(|(param, _)| names.tyvar_display_name(*param))
        .collect::<Vec<_>>();

    if params.is_empty() {
        format!("class {}", class_def.short_name)
    } else {
        format!("class {} {}", class_def.short_name, params.join(" "))
    }
}

fn pretty_scheme_detail(scheme: &Scheme, tables: PrettyTables<'_>) -> Option<String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scheme.pretty_with_tables(tables)
    }))
    .ok()
}

fn pretty_ty_detail(ty: &Ty, tables: PrettyTables<'_>) -> Option<String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ty.pretty_with_tables(tables)
    }))
    .ok()
}

fn import_qualifiers(
    session: &Session,
    _current_module: &ModuleName,
    import: &EffectiveImport,
) -> Vec<String> {
    let mut qualifiers = Vec::new();
    if import.qualifies {
        qualifiers.push(session.str(import.qualifier).to_string());
    }
    if import.accepts_module_name {
        qualifiers.push(import.module.as_string(session));
    }

    qualifiers.sort();
    qualifiers.dedup();
    qualifiers
}

fn collect_typed_expr_locals(
    file: FileId,
    expr: &TermExprNode<TcMetadata, Range<usize>>,
    tables: PrettyTables<'_>,
    out: &mut Vec<SourceCompletionCandidate>,
) {
    match &expr.node {
        TermExpr::Abs(param, annotation, body) => {
            let mut item =
                SourceCompletionItem::new(param.clone(), SourceCompletionItemKind::Variable);
            if let Some(ty) = annotation {
                item = item.with_optional_detail(pretty_ty_detail(ty, tables));
            }
            out.push(
                SourceCompletionCandidate::new(item)
                    .in_scope(file, Span::from_usize_range(body.anno.span.clone())),
            );
            collect_typed_expr_locals(file, body, tables, out);
        }
        TermExpr::Let(name, binder, in_expr) => {
            collect_typed_expr_locals(file, binder, tables, out);
            out.push(
                SourceCompletionCandidate::new(
                    SourceCompletionItem::new(name.clone(), SourceCompletionItemKind::Variable)
                        .with_optional_detail(pretty_ty_detail(&binder.anno.inferred_type, tables)),
                )
                .in_scope(file, Span::from_usize_range(in_expr.anno.span.clone())),
            );
            collect_typed_expr_locals(file, in_expr, tables, out);
        }
        TermExpr::App(lhs, rhs) => {
            collect_typed_expr_locals(file, lhs, tables, out);
            collect_typed_expr_locals(file, rhs, tables, out);
        }
        TermExpr::TyAbs(_, _, body)
        | TermExpr::EvAbs(_, _, body)
        | TermExpr::Anno(body, _)
        | TermExpr::TyApp(body, _)
        | TermExpr::EvApp(body, _) => {
            collect_typed_expr_locals(file, body, tables, out);
        }
        TermExpr::Case(scrutinee, arms) => {
            collect_typed_expr_locals(file, scrutinee, tables, out);
            for arm in arms {
                collect_typed_pattern_locals(
                    file,
                    &arm.node.pattern,
                    Span::from_usize_range(arm.node.expr.anno.span.clone()),
                    out,
                );
                collect_typed_expr_locals(file, &arm.node.expr, tables, out);
            }
        }
        TermExpr::Match(scrutinees, arms) => {
            for scrutinee in scrutinees {
                collect_typed_expr_locals(file, scrutinee, tables, out);
            }
            for arm in arms {
                for pattern in &arm.node.patterns {
                    collect_typed_pattern_locals(
                        file,
                        pattern,
                        Span::from_usize_range(arm.node.expr.anno.span.clone()),
                        out,
                    );
                }
                collect_typed_expr_locals(file, &arm.node.expr, tables, out);
            }
        }
        TermExpr::IfThenElse(cond, then_branch, else_branch) => {
            collect_typed_expr_locals(file, cond, tables, out);
            collect_typed_expr_locals(file, then_branch, tables, out);
            collect_typed_expr_locals(file, else_branch, tables, out);
        }
        TermExpr::Unit
        | TermExpr::Var(_)
        | TermExpr::Lit(_)
        | TermExpr::Hole
        | TermExpr::Con(_) => {}
    }
}

fn collect_typed_pattern_locals(
    file: FileId,
    pattern: &PatternNode<Range<usize>>,
    scope: Span,
    out: &mut Vec<SourceCompletionCandidate>,
) {
    match &pattern.node {
        Pattern::Var(name) => {
            out.push(
                SourceCompletionCandidate::new(SourceCompletionItem::new(
                    name.clone(),
                    SourceCompletionItemKind::Variable,
                ))
                .in_scope(file, scope),
            );
        }
        Pattern::Con(_, args) => {
            for arg in args {
                collect_typed_pattern_locals(file, arg, scope, out);
            }
        }
        Pattern::Lit(_) | Pattern::Wild => {}
    }
}
