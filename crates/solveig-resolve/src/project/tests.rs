use std::collections::HashMap;

use solveig_ast::ast::{
    ConstructorDef, CtorInfo, DataDefinition, Names, ResolvedModule, TyConDefinition, TyConId,
    TyConInfo,
};

use super::*;

#[test]
fn module_table_renders_def_paths_from_ids() {
    let session = Session::new();
    let sample = ModuleName(vec![session.intern("sample")]);
    let helper = ModuleName(vec![session.intern("sample"), session.intern("helper")]);
    let modules = ModuleTable::new(&session, [&sample, &helper]);
    let helper_id = modules
        .id(&helper)
        .expect("ICE: test module table must contain helper");
    let value = DefId {
        module: helper_id,
        name: session.intern("value"),
    };

    assert_eq!(
        modules.def_path(value).render(&session),
        "sample.helper.value"
    );
}

#[test]
fn typed_module_api_exports_table_backed_type_and_constructor_ids() {
    let session = Session::new();
    let module_id = ModuleId(7);
    let module_name = ModuleName(vec![session.intern("sample")]);
    let mut names = Names::new();
    let tycon_id = TyConId(42);
    let ctor_id = CtorId(43);
    let type_name = "sample.Widget";
    let ctor_name = "sample.MkWidget";
    let widget_ty = Ty::TyCon(tycon_id);
    let ctor_ty = Ty::make_arr(Ty::TyUnit, widget_ty.clone());

    let data_def = DataDefinition {
        name: type_name.to_string(),
        short_name: "Widget".to_string(),
        ty_params: Vec::new(),
        tycon_kind: Kind::Star,
        constructors: vec![ConstructorDef {
            id: ctor_id,
            name: ctor_name.to_string(),
            short_name: "MkWidget".to_string(),
            fields: vec![Ty::TyUnit],
            full_type: ctor_ty.clone(),
        }],
    };

    let mut tycon_table = TyConTable::new();
    tycon_table.insert_with_id(
        tycon_id,
        TyConInfo {
            name: names.symbol(type_name),
            short_name: names.symbol("Widget"),
            kind: Kind::Star,
            definition: TyConDefinition::Data(data_def.clone()),
        },
    );

    let mut ctor_table = CtorTable::new();
    ctor_table.insert_with_id(
        ctor_id,
        CtorInfo {
            name: names.symbol(ctor_name),
            short_name: names.symbol("MkWidget"),
            tycon: tycon_id,
            fields: vec![Ty::TyUnit],
            full_type: ctor_ty.clone(),
            tag: 0,
        },
    );

    let checked: CheckedModule = ResolvedModule {
        data_defs: vec![data_def],
        class_defs: Vec::new(),
        instance_defs: Vec::new(),
        builtin_defs: Vec::new(),
        tycon_table,
        ctor_table,
        class_table: ClassTable::new(),
        tycon_kinds: HashMap::from([(tycon_id, Kind::Star)]),
        constructor_types: HashMap::from([(ctor_id, ctor_ty.clone())]),
        func_schemes: HashMap::new(),
        func_defs: Vec::new(),
    };

    let api = build_typed_module_api(&session, &names, module_id, &module_name, &checked);
    let type_def = super::local_def_id(module_id, session.intern("Widget"));
    let ctor_def = super::local_def_id(module_id, session.intern("MkWidget"));

    assert_eq!(api.type_ids.get(&type_def), Some(&tycon_id));
    assert_eq!(api.constructor_ids.get(&ctor_def), Some(&ctor_id));
    assert_eq!(api.type_name(&names, type_def).as_deref(), Some(type_name));

    let mut tycon_kinds = HashMap::new();
    let mut constructor_types = HashMap::new();
    let mut func_schemes = HashMap::new();
    let mut class_defs = HashMap::new();
    let mut imported_tycons = TyConTable::new();
    let mut imported_ctors = CtorTable::new();
    let mut imported_classes = ClassTable::new();
    let mut instance_defs = Vec::new();
    let mut maps = TypedDisplayMaps {
        func_schemes: &mut func_schemes,
        tycon_kinds: &mut tycon_kinds,
        constructor_types: &mut constructor_types,
        class_defs: &mut class_defs,
        tycon_table: &mut imported_tycons,
        ctor_table: &mut imported_ctors,
        class_table: &mut imported_classes,
        instance_defs: &mut instance_defs,
    };

    api.extend_display_maps(&mut maps);

    let name_minted_tycon_id = names.tycon_id(type_name);
    let name_minted_ctor_id = names.ctor_id(ctor_name);

    assert_eq!(tycon_kinds.get(&tycon_id), Some(&Kind::Star));
    assert_eq!(constructor_types.get(&ctor_id), Some(&ctor_ty));
    assert!(tycon_kinds.keys().all(|id| *id != name_minted_tycon_id));
    assert!(
        constructor_types
            .keys()
            .all(|id| *id != name_minted_ctor_id)
    );
}
