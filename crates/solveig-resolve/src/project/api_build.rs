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

pub(super) fn directive_error(
    file: FileId,
    entity: &ModuleEntity,
    message: String,
) -> ResolveError {
    ResolveError::InvalidModuleDirective {
        span: FileSpan::new(file, entity.name.1),
        message,
    }
}

fn select_entity(
    api: &ModuleApi,
    file: FileId,
    entity: &ModuleEntity,
    names: &mut ApiNames,
) -> Result<(), ResolveError> {
    if entity.rename.is_some() {
        return Err(directive_error(
            file,
            entity,
            "renames are not allowed in export or private directives".to_string(),
        ));
    }

    let name = entity.name.0;
    let matches = usize::from(api.terms.contains_key(&name))
        + usize::from(api.types.contains_key(&name))
        + usize::from(api.type_aliases.contains_key(&name))
        + usize::from(api.classes.contains_key(&name));
    if matches == 0 {
        return Err(directive_error(
            file,
            entity,
            "module directive names no local term, type, alias, or class".to_string(),
        ));
    }
    if matches > 1 {
        return Err(directive_error(
            file,
            entity,
            "module directive name is ambiguous across namespaces".to_string(),
        ));
    }

    if api.terms.contains_key(&name) {
        if api
            .class_methods
            .values()
            .any(|methods| methods.contains(&name))
        {
            return Err(directive_error(
                file,
                entity,
                "a class method is exposed by selecting its owning class".to_string(),
            ));
        }
        if entity.exposure == EntityExposure::Constructors {
            return Err(directive_error(
                file,
                entity,
                "only a data type can expose constructors with `(..)`".to_string(),
            ));
        }
        names.terms.insert(name);
    } else if api.types.contains_key(&name) {
        names.types.insert(name);
        if entity.exposure == EntityExposure::Constructors {
            names.constructors.extend(
                api.data_constructors
                    .get(&name)
                    .into_iter()
                    .flatten()
                    .copied(),
            );
        }
    } else if api.type_aliases.contains_key(&name) {
        if entity.exposure == EntityExposure::Constructors {
            return Err(directive_error(
                file,
                entity,
                "a type alias has no constructors to expose".to_string(),
            ));
        }
        names.type_aliases.insert(name);
    } else if api.classes.contains_key(&name) {
        if entity.exposure == EntityExposure::Constructors {
            return Err(directive_error(
                file,
                entity,
                "a class has no constructors to expose".to_string(),
            ));
        }
        names.classes.insert(name);
        names
            .terms
            .extend(api.class_methods.get(&name).into_iter().flatten().copied());
    }
    Ok(())
}

/// Applies centralized local export/private declarations after compiler-owned
/// Prelude entries have been seeded into the module API.
pub fn apply_module_visibility(
    parsed: &ParsedModule,
    api: &mut ModuleApi,
) -> Result<(), ResolveError> {
    if let [_, duplicate, ..] = parsed.directives.exports.as_slice() {
        return Err(ResolveError::InvalidModuleDirective {
            span: FileSpan::new(parsed.file, duplicate.span),
            message: "a module may contain only one export directive".to_string(),
        });
    }
    if let [_, duplicate, ..] = parsed.directives.private.as_slice() {
        return Err(ResolveError::InvalidModuleDirective {
            span: FileSpan::new(parsed.file, duplicate.span),
            message: "a module may contain only one private directive".to_string(),
        });
    }

    if let Some(exports) = parsed.directives.exports.first() {
        let mut external = ApiNames::default();
        for entity in &exports.entities {
            select_entity(api, parsed.file, entity, &mut external)?;
        }
        api.external = external;
    }
    if let Some(private) = parsed.directives.private.first() {
        let mut private_names = ApiNames::default();
        for entity in &private.entities {
            select_entity(api, parsed.file, entity, &mut private_names)?;
        }
        api.private = private_names;
    }

    let exported_private = api
        .external
        .terms
        .intersection(&api.private.terms)
        .next()
        .or_else(|| api.external.types.intersection(&api.private.types).next())
        .or_else(|| {
            api.external
                .type_aliases
                .intersection(&api.private.type_aliases)
                .next()
        })
        .or_else(|| {
            api.external
                .constructors
                .intersection(&api.private.constructors)
                .next()
        })
        .or_else(|| {
            api.external
                .classes
                .intersection(&api.private.classes)
                .next()
        });
    if exported_private.is_some() {
        return Err(ResolveError::InvalidModuleDirective {
            span: FileSpan::new(
                parsed.file,
                parsed
                    .directives
                    .private
                    .first()
                    .expect("ICE: exported private name requires private directive")
                    .span,
            ),
            message: "the same entity cannot be both exported and private".to_string(),
        });
    }

    Ok(())
}

fn reexport_collision(file: FileId, reexport: &ReexportDecl, name: Symbol) -> ResolveError {
    ResolveError::ReexportConflict {
        span: FileSpan::new(file, reexport.span),
        name,
    }
}

fn insert_reexported(
    target: &mut FxHashMap<Symbol, DefId>,
    name: Symbol,
    id: DefId,
    file: FileId,
    reexport: &ReexportDecl,
) -> Result<(), ResolveError> {
    if let Some(previous) = target.insert(name, id)
        && previous != id
    {
        return Err(reexport_collision(file, reexport, name));
    }
    Ok(())
}

fn merge_reexport(
    api: &mut ModuleApi,
    projected: &ModuleApi,
    file: FileId,
    reexport: &ReexportDecl,
) -> Result<(), ResolveError> {
    for (name, id) in &projected.terms {
        insert_reexported(&mut api.terms, *name, *id, file, reexport)?;
        api.external.terms.insert(*name);
        api.reexported.terms.insert(*name);
    }
    for (name, id) in &projected.types {
        insert_reexported(&mut api.types, *name, *id, file, reexport)?;
        api.external.types.insert(*name);
        api.reexported.types.insert(*name);
    }
    for (name, id) in &projected.type_aliases {
        insert_reexported(&mut api.type_aliases, *name, *id, file, reexport)?;
        api.external.type_aliases.insert(*name);
        api.reexported.type_aliases.insert(*name);
    }
    for (name, id) in &projected.constructors {
        insert_reexported(&mut api.constructors, *name, *id, file, reexport)?;
        api.external.constructors.insert(*name);
        api.reexported.constructors.insert(*name);
    }
    for (name, id) in &projected.classes {
        insert_reexported(&mut api.classes, *name, *id, file, reexport)?;
        api.external.classes.insert(*name);
        api.reexported.classes.insert(*name);
    }
    api.fixities.extend(projected.fixities.clone());
    api.docs.extend(projected.docs.clone());
    api.class_methods.extend(projected.class_methods.clone());
    api.data_constructors
        .extend(projected.data_constructors.clone());
    Ok(())
}

/// Resolves and composes all centralized re-export declarations after local API
/// construction. Module order is dependency-first, so every source API is ready.
pub fn apply_module_reexports(
    parsed: &ParsedModule,
    api: &mut ModuleApi,
    dependencies: &FxHashMap<ModuleName, SharedModuleApi>,
) -> Result<(), ResolveError> {
    let mut seen_modules = FxHashSet::default();
    for reexport in &parsed.directives.reexports {
        if !seen_modules.insert(reexport.module.clone()) {
            return Err(ResolveError::InvalidModuleDirective {
                span: FileSpan::new(parsed.file, reexport.span),
                message: "a source module may be re-exported only once".to_string(),
            });
        }
        let source =
            dependencies
                .get(&reexport.module)
                .ok_or_else(|| ResolveError::UnknownModule {
                    span: FileSpan::new(parsed.file, reexport.span),
                    module: reexport.module.clone(),
                })?;
        let visible = source.visible_to(api.package);
        let projected =
            project_module_selection(&visible, parsed.file, &reexport.module, &reexport.selection)?;
        merge_reexport(api, &projected, parsed.file, reexport)?;
    }
    Ok(())
}

pub fn build_module_api(
    package: PackageId,
    module_id: ModuleId,
    parsed: &ParsedModule,
) -> Result<ModuleApi, ResolveError> {
    let mut api = ModuleApi::new(package);
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
                    api.data_constructors
                        .entry(name.0)
                        .or_default()
                        .push(ctor.0.name.0);
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
    module_api: &ModuleApi,
) -> TypedModuleApi {
    let mut api = TypedModuleApi::new(module_api.package);
    let prefix = module_name.as_string(session) + ".";

    api.tycon_table = checked.tycon_table.clone();
    api.ctor_table = checked.ctor_table.clone();
    api.class_table = checked.class_table.clone();
    api.type_aliases = type_aliases;
    api.docs = module_api.docs.clone();

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
