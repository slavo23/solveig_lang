//! Source-level code action collection for editor integrations.

use super::*;
use std::fs;

pub(super) fn code_actions_in_compilation(
    source_map: &SourceMap,
    actions: &[SourceCodeAction],
    file: &Path,
    request_span: Span,
) -> Vec<SourceCodeAction> {
    let Some(file_id) = source_lookup::source_file_id_in_source_map(source_map, file) else {
        return Vec::new();
    };

    actions
        .iter()
        .filter(|action| {
            let trigger = action.trigger_span();
            trigger.file == file_id && spans_touch(trigger.span, request_span)
        })
        .cloned()
        .collect()
}

pub(super) fn hole_code_actions(
    holes: &[HoleInfo],
    constructor_types: &FxHashMap<CtorId, Ty>,
    ctor_table: &CtorTable,
    tables: PrettyTables<'_>,
) -> Vec<SourceCodeAction> {
    let mut out = Vec::new();

    for hole in holes {
        let mut seen = Vec::<String>::new();
        for suggestion in hole.suggestions(constructor_types, ctor_table, tables.names) {
            if seen.contains(&suggestion) {
                continue;
            }
            seen.push(suggestion.clone());
            out.push(SourceCodeAction::new(
                format!("Fill hole with `{suggestion}`"),
                hole.span,
                vec![SourceTextEdit::new(
                    hole.span.file,
                    hole.span.span,
                    suggestion,
                )],
            ));
        }
    }

    out
}

pub(super) fn resolve_code_actions(
    session: &Session,
    source_root: &Path,
    source_provider: &SourceProvider,
    current_module: &ModuleName,
    parsed: &ParsedModule,
    apis: &FxHashMap<ModuleName, SharedModuleApi>,
    errors: &[ResolveError],
) -> Vec<SourceCodeAction> {
    let mut out = Vec::new();

    for error in errors {
        let ResolveError::UnknownName { span, kind, name } = error else {
            continue;
        };
        if name.contains('.') {
            continue;
        }

        let source_map = session.source_map();
        let name_symbol = session.intern(name);
        let mut candidates = apis
            .iter()
            .filter(|(module_name, _)| *module_name != current_module)
            .filter(|(module_name, _)| !is_compiler_prelude_module(session, module_name))
            .filter(|(_, api)| api_exports_name(api, name_symbol, *kind))
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(module_name, _)| module_name.as_string(session));
        let mut seen_modules = Vec::new();

        for (module_name, _) in candidates {
            seen_modules.push(module_name.as_string(session));
            if let Some(action) =
                missing_import_action(session, &source_map, parsed, *span, module_name, name)
            {
                out.push(action);
            }
        }
        let mut excluded_modules = vec![current_module.as_string(session)];
        excluded_modules.extend(seen_modules);

        out.extend(scanned_missing_import_actions(
            source_root,
            source_provider,
            parsed,
            *span,
            name,
            *kind,
            &excluded_modules,
        ));
    }

    out
}

pub(super) fn type_error_code_actions(
    source_map: &SourceMap,
    errors: &[TypeError],
) -> Vec<SourceCodeAction> {
    let mut out = Vec::new();

    for error in errors {
        let TypeError::MissingInstanceMethod {
            span,
            method,
            class_name,
        } = error
        else {
            continue;
        };
        let Some(edit) = missing_instance_method_edit(source_map, *span, method) else {
            continue;
        };

        out.push(SourceCodeAction::new(
            format!("Add `{method}` stub for `{class_name}`"),
            *span,
            vec![edit],
        ));
    }

    out
}

pub(super) fn inferred_signature_code_actions(
    session: &Session,
    parsed: &ParsedModule,
    module_id: ModuleId,
    checked: &CheckedModule,
    tables: PrettyTables<'_>,
) -> Vec<SourceCodeAction> {
    let signatures = parsed
        .defs
        .iter()
        .filter_map(|defn| match &defn.0 {
            SurfaceDefinition::SigDef { name, .. } => Some(name.0),
            _ => None,
        })
        .collect::<FxHashSet<_>>();
    let mut out = Vec::new();

    for defn in &parsed.defs {
        let SurfaceDefinition::FunDef { name, .. } = &defn.0 else {
            continue;
        };
        if signatures.contains(&name.0) {
            continue;
        }

        let def_id = DefId {
            module: module_id,
            name: name.0,
        };
        let Some(scheme) = checked.func_schemes.get(&def_id) else {
            continue;
        };
        let function_name = session.str(name.0).to_string();
        let signature = scheme.pretty_with_tables(tables);
        let insertion = Span::new(defn.1.start, defn.1.start);

        out.push(
            SourceCodeAction::new(
                format!("Add type signature for `{function_name}`"),
                FileSpan::new(parsed.file, name.1),
                vec![SourceTextEdit::new(
                    parsed.file,
                    insertion,
                    format!("{function_name} : {signature}\n"),
                )],
            )
            .with_category(SourceCodeActionCategory::InferredSignature),
        );
    }

    out
}

fn spans_touch(left: Span, right: Span) -> bool {
    if right.is_empty() {
        return left.start <= right.start && right.start <= left.end;
    }
    if left.is_empty() {
        return right.start <= left.start && left.start <= right.end;
    }

    left.start < right.end && right.start < left.end
}

fn api_exports_name(api: &ModuleApi, name: Symbol, kind: ResolveNameKind) -> bool {
    match kind {
        ResolveNameKind::Term | ResolveNameKind::Signature => api.terms.contains_key(&name),
        ResolveNameKind::Operator | ResolveNameKind::Fixity => {
            api.terms.contains_key(&name) || api.fixities.contains_key(&name)
        }
        ResolveNameKind::TypeConstructor => api.types.contains_key(&name),
        ResolveNameKind::TypeAlias => api.type_aliases.contains_key(&name),
        ResolveNameKind::Constructor => api.constructors.contains_key(&name),
        ResolveNameKind::Class => api.classes.contains_key(&name),
    }
}

fn api_exports_name_text(
    session: &Session,
    api: &ModuleApi,
    name: &str,
    kind: ResolveNameKind,
) -> bool {
    match kind {
        ResolveNameKind::Term | ResolveNameKind::Signature => {
            contains_symbol_text(session, api.terms.keys(), name)
        }
        ResolveNameKind::Operator | ResolveNameKind::Fixity => {
            contains_symbol_text(session, api.terms.keys(), name)
                || contains_symbol_text(session, api.fixities.keys(), name)
        }
        ResolveNameKind::TypeConstructor => contains_symbol_text(session, api.types.keys(), name),
        ResolveNameKind::TypeAlias => contains_symbol_text(session, api.type_aliases.keys(), name),
        ResolveNameKind::Constructor => {
            contains_symbol_text(session, api.constructors.keys(), name)
        }
        ResolveNameKind::Class => contains_symbol_text(session, api.classes.keys(), name),
    }
}

fn contains_symbol_text<'a>(
    session: &Session,
    mut symbols: impl Iterator<Item = &'a Symbol>,
    name: &str,
) -> bool {
    symbols.any(|symbol| &*session.str(*symbol) == name)
}

fn missing_import_action(
    session: &Session,
    source_map: &SourceMap,
    parsed: &ParsedModule,
    trigger: FileSpan,
    module_name: &ModuleName,
    name: &str,
) -> Option<SourceCodeAction> {
    if let Some(import) = parsed
        .imports
        .iter()
        .find(|import| &import.module == module_name)
    {
        return missing_import_list_action(session, source_map, parsed.file, trigger, import, name);
    }

    let module_text = module_name.as_string(session);
    Some(new_import_action(parsed, trigger, &module_text, name))
}

fn scanned_missing_import_actions(
    source_root: &Path,
    source_provider: &SourceProvider,
    parsed: &ParsedModule,
    trigger: FileSpan,
    name: &str,
    kind: ResolveNameKind,
    excluded_modules: &[String],
) -> Vec<SourceCodeAction> {
    let mut files = Vec::new();
    collect_solveig_files(source_root, &mut files);

    let mut out = Vec::new();
    let mut seen = excluded_modules.to_vec();

    for file in files {
        let Some(module_text) =
            scanned_module_exports_name(source_root, source_provider, &file, name, kind)
        else {
            continue;
        };
        if seen.contains(&module_text) {
            continue;
        }

        seen.push(module_text.clone());
        out.push(new_import_action(parsed, trigger, &module_text, name));
    }

    out
}

fn scanned_module_exports_name(
    source_root: &Path,
    source_provider: &SourceProvider,
    file: &Path,
    name: &str,
    kind: ResolveNameKind,
) -> Option<String> {
    let source = source_provider.read_to_string(file).ok()?;
    let session = Session::new();
    let module_name = crate::project::module_name_from_file(&session, source_root, file).ok()?;
    let file_id = session.add_file(file.to_path_buf(), source.clone());
    let parsed = solveig_syntax::parser::parse_source(&session, file_id, &source);
    if session.diagnostics().has_errors() {
        return None;
    }

    let api = build_module_api(ModuleId(0), &parsed).ok()?;
    api_exports_name_text(&session, &api, name, kind).then(|| module_name.as_string(&session))
}

fn collect_solveig_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_solveig_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "sv") {
            out.push(path);
        }
    }

    out.sort();
}

fn new_import_action(
    parsed: &ParsedModule,
    trigger: FileSpan,
    module_text: &str,
    name: &str,
) -> SourceCodeAction {
    let span = new_import_insert_span(parsed);
    let new_text = if parsed.imports.is_empty() {
        format!("import {module_text}({name})\n\n")
    } else {
        format!("\nimport {module_text}({name})")
    };

    SourceCodeAction::new(
        format!("Import `{name}` from `{module_text}`"),
        trigger,
        vec![SourceTextEdit::new(parsed.file, span, new_text)],
    )
}

fn missing_import_list_action(
    session: &Session,
    source_map: &SourceMap,
    file: FileId,
    trigger: FileSpan,
    import: &ImportDecl,
    name: &str,
) -> Option<SourceCodeAction> {
    let ImportSpec::Only(names) = &import.spec else {
        return None;
    };
    let name_symbol = session.intern(name);
    if names.contains(&name_symbol) {
        return None;
    }

    let offset = import_list_close_paren(source_map, file, import.span)?;
    let span = Span::new(offset, offset);
    let module_text = import.module.as_string(session);

    Some(SourceCodeAction::new(
        format!("Add `{name}` to import `{module_text}`"),
        trigger,
        vec![SourceTextEdit::new(file, span, format!(", {name}"))],
    ))
}

fn import_list_close_paren(source_map: &SourceMap, file: FileId, span: Span) -> Option<u32> {
    let source = source_map.files().get(file.0 as usize)?;
    let start = span.start as usize;
    let end = span.end.min(u32::try_from(source.src.len()).ok()?) as usize;
    let text = source.src.get(start..end)?;
    let close = text.rfind(')')?;

    Some(span.start.saturating_add(u32::try_from(close).ok()?))
}

fn new_import_insert_span(parsed: &ParsedModule) -> Span {
    let offset = parsed.imports.last().map_or(0, |import| import.span.end);
    Span::new(offset, offset)
}

fn missing_instance_method_edit(
    source_map: &SourceMap,
    instance_span: FileSpan,
    method: &str,
) -> Option<SourceTextEdit> {
    let source = source_map.files().get(instance_span.file.0 as usize)?;
    let offset = instance_method_insert_offset(&source.src, instance_span.span);
    let span = Span::new(offset, offset);

    Some(SourceTextEdit::new(
        instance_span.file,
        span,
        format!("\n    {method} = ??\n"),
    ))
}

fn instance_method_insert_offset(source: &str, span: Span) -> u32 {
    let start = span.start as usize;
    let mut end = span.end as usize;
    end = end.min(source.len());

    let bytes = source.as_bytes();
    let mut before_trivia = end;
    while before_trivia > start && bytes[before_trivia - 1].is_ascii_whitespace() {
        before_trivia -= 1;
    }

    if before_trivia > start && bytes[before_trivia - 1] == b'}' {
        u32::try_from(before_trivia - 1).unwrap_or(span.end)
    } else {
        u32::try_from(end).unwrap_or(span.end)
    }
}
