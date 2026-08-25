//! Exact, entity-aware projections shared by imports and re-exports.

use super::api_build::directive_error;
use super::*;

fn copy_doc(source: &ModuleApi, target: &mut ModuleApi, id: DefId) {
    if let Some(docs) = source.docs.get(&id) {
        target.docs.insert(id, docs.clone());
    }
}

fn insert_selected(
    target: &mut FxHashMap<Symbol, DefId>,
    name: Symbol,
    id: DefId,
    file: FileId,
    entity: &ModuleEntity,
) -> Result<(), ResolveError> {
    if let Some(previous) = target.get(&name)
        && *previous != id
    {
        return Err(directive_error(
            file,
            entity,
            "module selection introduces two definitions under the same name".to_string(),
        ));
    }
    target.insert(name, id);
    Ok(())
}

fn project_module_entity(
    source: &ModuleApi,
    out: &mut ModuleApi,
    file: FileId,
    module: &ModuleName,
    entity: &ModuleEntity,
) -> Result<(), ResolveError> {
    let name = entity.name.0;
    let public_name = entity.rename.map_or(name, |rename| rename.0);
    let matches = usize::from(source.terms.contains_key(&name))
        + usize::from(source.types.contains_key(&name))
        + usize::from(source.type_aliases.contains_key(&name))
        + usize::from(source.classes.contains_key(&name));
    if matches == 0 {
        if source.constructors.contains_key(&name) {
            return Err(directive_error(
                file,
                entity,
                "a constructor is selected through its owning data type with `T(..)`".to_string(),
            ));
        }
        return Err(ResolveError::UnknownImportName {
            span: FileSpan::new(file, entity.name.1),
            module: module.clone(),
            name,
        });
    }
    if matches > 1 {
        return Err(directive_error(
            file,
            entity,
            "module selection name is ambiguous across namespaces".to_string(),
        ));
    }

    if let Some(id) = source.terms.get(&name).copied() {
        if source
            .class_methods
            .values()
            .any(|methods| methods.contains(&name))
        {
            return Err(directive_error(
                file,
                entity,
                "a class method is selected through its owning class".to_string(),
            ));
        }
        if entity.exposure == EntityExposure::Constructors {
            return Err(directive_error(
                file,
                entity,
                "only a data type can expose constructors with `(..)`".to_string(),
            ));
        }
        insert_selected(&mut out.terms, public_name, id, file, entity)?;
        copy_doc(source, out, id);
        if let Some(fixity) = source.fixities.get(&name) {
            out.fixities.insert(public_name, *fixity);
        }
        return Ok(());
    }

    if let Some(id) = source.types.get(&name).copied() {
        insert_selected(&mut out.types, public_name, id, file, entity)?;
        copy_doc(source, out, id);
        if entity.exposure == EntityExposure::Constructors {
            let constructors = source
                .data_constructors
                .get(&name)
                .cloned()
                .unwrap_or_default();
            for constructor in &constructors {
                let constructor_id = source
                    .constructors
                    .get(constructor)
                    .copied()
                    .unwrap_or_else(|| solveig_base::ice!("data API names a missing constructor"));
                insert_selected(
                    &mut out.constructors,
                    *constructor,
                    constructor_id,
                    file,
                    entity,
                )?;
                copy_doc(source, out, constructor_id);
            }
            out.data_constructors.insert(public_name, constructors);
        }
        return Ok(());
    }

    if let Some(id) = source.type_aliases.get(&name).copied() {
        if entity.exposure == EntityExposure::Constructors {
            return Err(directive_error(
                file,
                entity,
                "a type alias has no constructors to expose".to_string(),
            ));
        }
        insert_selected(&mut out.type_aliases, public_name, id, file, entity)?;
        copy_doc(source, out, id);
        return Ok(());
    }

    let id = source.classes.get(&name).copied().unwrap_or_else(|| {
        solveig_base::ice!("module entity matched a class but no class was present")
    });
    if entity.exposure == EntityExposure::Constructors {
        return Err(directive_error(
            file,
            entity,
            "a class has no constructors to expose".to_string(),
        ));
    }
    insert_selected(&mut out.classes, public_name, id, file, entity)?;
    copy_doc(source, out, id);
    let methods = source.class_methods.get(&name).cloned().unwrap_or_default();
    for method in &methods {
        let method_id = source
            .terms
            .get(method)
            .copied()
            .unwrap_or_else(|| solveig_base::ice!("class API names a missing method term"));
        insert_selected(&mut out.terms, *method, method_id, file, entity)?;
        copy_doc(source, out, method_id);
        if let Some(fixity) = source.fixities.get(method) {
            out.fixities.insert(*method, *fixity);
        }
    }
    out.class_methods.insert(public_name, methods);
    Ok(())
}

/// Projects one module API through an exact import or re-export selection.
pub fn project_module_selection(
    source: &ModuleApi,
    file: FileId,
    module: &ModuleName,
    selection: &ModuleSelection,
) -> Result<ModuleApi, ResolveError> {
    project_module_selection_diagnostics(source, file, module, selection)
        .map_err(|mut errors| errors.remove(0))
}

/// Projects a selection while retaining every independent entity diagnostic.
pub fn project_module_selection_diagnostics(
    source: &ModuleApi,
    file: FileId,
    module: &ModuleName,
    selection: &ModuleSelection,
) -> Result<ModuleApi, Vec<ResolveError>> {
    match selection {
        ModuleSelection::All => Ok(source.clone()),
        ModuleSelection::Only(entities) => {
            let mut out = ModuleApi::new(source.package);
            let mut errors = Vec::new();
            for entity in entities {
                if let Err(error) = project_module_entity(source, &mut out, file, module, entity) {
                    errors.push(error);
                }
            }
            if errors.is_empty() {
                Ok(out)
            } else {
                Err(errors)
            }
        }
    }
}
