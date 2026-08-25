struct ImportedTypeInfo {
    func_schemes: FxHashMap<DefId, Scheme>,
    tycon_kinds: FxHashMap<TyConId, solveig_ast::ast::Kind>,
    constructor_types: FxHashMap<CtorId, solveig_ast::ast::Ty>,
    class_defs: FxHashMap<ClassId, ClassDefinition>,
    tycon_table: TyConTable,
    ctor_table: CtorTable,
    class_table: ClassTable,
}

fn collect_imported_type_info(imports: &[ImportedTypedApi]) -> ImportedTypeInfo {
    let mut func_schemes = FxHashMap::default();
    let mut tycon_kinds = FxHashMap::default();
    let mut constructor_types = FxHashMap::default();
    let mut class_defs = FxHashMap::default();
    let mut tycon_table = TyConTable::new();
    let mut ctor_table = CtorTable::new();
    let mut class_table = ClassTable::new();

    for imp in imports {
        let mut display_maps = TypedDisplayMaps {
            func_schemes: &mut func_schemes,
            tycon_kinds: &mut tycon_kinds,
            constructor_types: &mut constructor_types,
            class_defs: &mut class_defs,
            tycon_table: &mut tycon_table,
            ctor_table: &mut ctor_table,
            class_table: &mut class_table,
        };

        imp.api.extend_display_maps(&mut display_maps);
    }

    ImportedTypeInfo {
        func_schemes,
        tycon_kinds,
        constructor_types,
        class_defs,
        tycon_table,
        ctor_table,
        class_table,
    }
}
