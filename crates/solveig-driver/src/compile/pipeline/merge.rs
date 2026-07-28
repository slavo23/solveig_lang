pub(super) fn merge_checked_modules(
    mods: Vec<CheckedModule>,
    names: &mut Names,
) -> Result<CheckedModule, DriverError> {
    let mut data_defs = Vec::new();
    let mut class_defs = Vec::new();
    let mut instance_defs = Vec::new();
    let mut builtin_defs = Vec::new();
    let mut tycon_kinds = FxHashMap::default();
    let mut constructor_types = FxHashMap::default();
    let mut func_schemes = FxHashMap::default();
    let mut func_defs = Vec::new();

    for m in mods {
        let CheckedModule {
            data_defs: module_data_defs,
            class_defs: module_class_defs,
            instance_defs: module_instance_defs,
            builtin_defs: module_builtin_defs,
            tycon_table: module_tycon_table,
            ctor_table: module_ctor_table,
            class_table: _,
            tycon_kinds: module_tycon_kinds,
            constructor_types: module_constructor_types,
            func_schemes: module_func_schemes,
            func_defs: module_func_defs,
        } = m;

        data_defs.extend(module_data_defs);
        class_defs.extend(module_class_defs);
        instance_defs.extend(module_instance_defs);
        builtin_defs.extend(module_builtin_defs);
        extend_unique_tycon(
            &mut tycon_kinds,
            module_tycon_kinds,
            &module_tycon_table,
            names,
        )?;
        extend_unique_ctor(
            &mut constructor_types,
            module_constructor_types,
            &module_ctor_table,
            names,
        )?;
        extend_unique_def(&mut func_schemes, module_func_schemes, "function", names)?;
        func_defs.extend(module_func_defs);
    }

    let mut module = solveig_ast::ast::ResolvedModule {
        data_defs,
        class_defs,
        instance_defs,
        builtin_defs,
        tycon_table: Default::default(),
        ctor_table: Default::default(),
        class_table: Default::default(),
        tycon_kinds,
        constructor_types,
        func_schemes,
        func_defs,
    };
    module.rebuild_identity_tables(names);

    Ok(module)
}

fn extend_unique_def<T>(
    dst: &mut FxHashMap<DefId, T>,
    src: FxHashMap<DefId, T>,
    label: &'static str,
    names: &Names,
) -> Result<(), DriverError> {
    for (k, v) in src {
        if dst.insert(k, v).is_some() {
            return Err(DriverError::DuplicateDefinition {
                kind: label,
                name: names.def_display_name(k),
            });
        }
    }
    Ok(())
}

fn extend_unique_tycon(
    dst: &mut FxHashMap<TyConId, solveig_ast::ast::Kind>,
    src: FxHashMap<TyConId, solveig_ast::ast::Kind>,
    tycons: &TyConTable,
    names: &Names,
) -> Result<(), DriverError> {
    for (k, v) in src {
        if dst.insert(k, v).is_some() {
            return Err(DriverError::DuplicateDefinition {
                kind: "type constructor",
                name: tycon_name_for_error(k, tycons, names),
            });
        }
    }
    Ok(())
}

fn extend_unique_ctor(
    dst: &mut FxHashMap<CtorId, Ty>,
    src: FxHashMap<CtorId, Ty>,
    ctors: &CtorTable,
    names: &Names,
) -> Result<(), DriverError> {
    for (k, v) in src {
        if dst.insert(k, v).is_some() {
            return Err(DriverError::DuplicateDefinition {
                kind: "constructor",
                name: ctor_name_for_error(k, ctors, names),
            });
        }
    }
    Ok(())
}

fn tycon_name_for_error(id: TyConId, tycons: &TyConTable, names: &Names) -> String {
    tycons.get(id).map_or_else(
        || format!("T{}", id.0),
        |info| names.symbol_display_name(info.name),
    )
}

fn ctor_name_for_error(id: CtorId, ctors: &CtorTable, names: &Names) -> String {
    ctors.get(id).map_or_else(
        || format!("C{}", id.0),
        |info| names.symbol_display_name(info.name),
    )
}
