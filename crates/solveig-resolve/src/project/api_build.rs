//! Module API construction from parsed declarations.
//! The resolver uses these APIs for imports, duplicate detection, and visibility.

use super::*;

fn file_span(file: FileId, span: Span) -> FileSpan {
    FileSpan::new(file, span)
}

fn check_duplicate(
    seen: &mut FxHashMap<Symbol, FileSpan>,
    file: FileId,
    span: Span,
    kind: ResolveNameKind,
    name: Symbol,
) -> Result<(), ResolveError> {
    let current = file_span(file, span);
    if let Some(previous) = seen.insert(name, current) {
        return Err(ResolveError::DuplicateDefinition {
            span: current,
            previous: Some(previous),
            kind,
            name,
        });
    }

    Ok(())
}

fn insert_docs(api: &mut ModuleApi, def_id: DefId, docs: Option<&DocComment>) {
    if let Some(docs) = docs {
        api.docs.entry(def_id).or_insert_with(|| docs.clone());
    }
}

pub fn build_module_api(
    module_id: ModuleId,
    parsed: &ParsedModule,
) -> Result<ModuleApi, ResolveError> {
    let mut api = ModuleApi::default();
    let function_groups = function_equation_group_indices(parsed)?;
    let function_group_starts = function_groups
        .iter()
        .filter_map(|group| group.first().copied())
        .collect::<FxHashSet<_>>();
    let mut term_defs = FxHashMap::<Symbol, FileSpan>::default();
    let mut term_sigs = FxHashMap::<Symbol, FileSpan>::default();
    let mut types = FxHashMap::<Symbol, FileSpan>::default();
    let mut type_aliases = FxHashMap::<Symbol, FileSpan>::default();
    let mut seen_constructors = FxHashMap::<Symbol, FileSpan>::default();
    let mut classes = FxHashMap::<Symbol, FileSpan>::default();
    let mut fixity_defs = FxHashMap::<Symbol, FileSpan>::default();

    for (def_index, def) in parsed.defs.iter().enumerate() {
        match &def.0 {
            SurfaceDefinition::FunDef { name, docs, .. } => {
                if !function_group_starts.contains(&def_index) {
                    continue;
                }

                check_duplicate(
                    &mut term_defs,
                    parsed.file,
                    name.1,
                    ResolveNameKind::Term,
                    name.0,
                )?;

                let def_id = local_def_id(module_id, name.0);
                api.terms.insert(name.0, def_id);
                insert_docs(&mut api, def_id, docs.as_ref());
            }

            SurfaceDefinition::SigDef { name, docs, .. } => {
                check_duplicate(
                    &mut term_sigs,
                    parsed.file,
                    name.1,
                    ResolveNameKind::Signature,
                    name.0,
                )?;

                let def_id = local_def_id(module_id, name.0);
                api.terms.entry(name.0).or_insert(def_id);
                insert_docs(&mut api, def_id, docs.as_ref());
            }

            SurfaceDefinition::DataDef {
                name,
                constructors,
                docs,
                ..
            } => {
                check_duplicate(
                    &mut types,
                    parsed.file,
                    name.1,
                    ResolveNameKind::TypeConstructor,
                    name.0,
                )?;

                let def_id = local_def_id(module_id, name.0);
                api.types.insert(name.0, def_id);
                insert_docs(&mut api, def_id, docs.as_ref());

                if let Some(previous) = type_aliases.get(&name.0).copied() {
                    return Err(ResolveError::DuplicateDefinition {
                        span: file_span(parsed.file, name.1),
                        previous: Some(previous),
                        kind: ResolveNameKind::TypeConstructor,
                        name: name.0,
                    });
                }

                for ctor in constructors {
                    check_duplicate(
                        &mut seen_constructors,
                        parsed.file,
                        ctor.0.name.1,
                        ResolveNameKind::Constructor,
                        ctor.0.name.0,
                    )?;

                    let ctor_id = local_def_id(module_id, ctor.0.name.0);
                    api.constructors.insert(ctor.0.name.0, ctor_id);
                    insert_docs(&mut api, ctor_id, ctor.0.docs.as_ref());
                }
            }

            SurfaceDefinition::TypeAliasDef { name, docs, .. } => {
                check_duplicate(
                    &mut type_aliases,
                    parsed.file,
                    name.1,
                    ResolveNameKind::TypeAlias,
                    name.0,
                )?;

                if let Some(previous) = types.get(&name.0).copied() {
                    return Err(ResolveError::DuplicateDefinition {
                        span: file_span(parsed.file, name.1),
                        previous: Some(previous),
                        kind: ResolveNameKind::TypeAlias,
                        name: name.0,
                    });
                }

                let def_id = local_def_id(module_id, name.0);
                api.type_aliases.insert(name.0, def_id);
                insert_docs(&mut api, def_id, docs.as_ref());
            }

            SurfaceDefinition::ClassDef {
                name,
                methods,
                docs,
                ..
            } => {
                let class_name = name.0;
                check_duplicate(
                    &mut classes,
                    parsed.file,
                    name.1,
                    ResolveNameKind::Class,
                    class_name,
                )?;

                let class_id = local_def_id(module_id, class_name);
                api.classes.insert(class_name, class_id);
                insert_docs(&mut api, class_id, docs.as_ref());

                let mut method_names = Vec::new();

                for method in methods {
                    let method_name = method.name.0;
                    check_duplicate(
                        &mut term_defs,
                        parsed.file,
                        method.name.1,
                        ResolveNameKind::Term,
                        method_name,
                    )?;

                    let method_id = local_def_id(module_id, method_name);
                    api.terms.entry(method_name).or_insert(method_id);
                    insert_docs(&mut api, method_id, method.docs.as_ref());

                    method_names.push(method_name);
                }

                api.class_methods.insert(class_name, method_names);
            }

            SurfaceDefinition::FixityDef { fixity, ops, .. } => {
                for op in ops {
                    check_duplicate(
                        &mut fixity_defs,
                        parsed.file,
                        op.1,
                        ResolveNameKind::Fixity,
                        op.0,
                    )?;

                    api.fixities.insert(op.0, *fixity);
                }
            }

            SurfaceDefinition::InstanceDef { .. } => {}
        }
    }

    Ok(api)
}

pub fn build_typed_module_api(
    session: &Session,
    names: &Names,
    module_id: ModuleId,
    module_name: &ModuleName,
    checked: &CheckedModule,
    type_aliases: FxHashMap<DefId, TypeAlias>,
    docs: FxHashMap<DefId, DocComment>,
) -> TypedModuleApi {
    let mut api = TypedModuleApi::default();
    let prefix = module_name.as_string(session) + ".";

    api.tycon_table = checked.tycon_table.clone();
    api.ctor_table = checked.ctor_table.clone();
    api.class_table = checked.class_table.clone();
    api.type_aliases = type_aliases;
    api.docs = docs;

    for (id, scheme) in &checked.func_schemes {
        if id.module == module_id {
            api.term_schemes.insert(*id, scheme.clone());
        }
    }

    for (id, kind) in &checked.tycon_kinds {
        let Some(info) = checked.tycon_table.get(*id) else {
            continue;
        };
        let name = names.symbol_display_name(info.name);
        if let Some(stripped) = name.strip_prefix(&prefix) {
            let def_id = local_def_id(module_id, session.intern(stripped));
            api.type_kinds.insert(def_id, kind.clone());
            api.type_ids.insert(def_id, *id);
        }
    }

    for (id, ty) in &checked.constructor_types {
        let Some(info) = checked.ctor_table.get(*id) else {
            continue;
        };
        let name = names.symbol_display_name(info.name);
        if let Some(stripped) = name.strip_prefix(&prefix) {
            let def_id = local_def_id(module_id, session.intern(stripped));
            api.constructor_types.insert(def_id, ty.clone());
            api.constructor_ids.insert(def_id, *id);
        }
    }

    for class_def in &checked.class_defs {
        let stripped = class_def
            .name
            .strip_prefix(&prefix)
            .unwrap_or(&class_def.name);

        api.class_defs.insert(
            local_def_id(module_id, session.intern(stripped)),
            class_def.clone(),
        );
    }

    api.instance_defs = checked.instance_defs.clone();

    api
}
