//! Project API and module-table regression tests.
//! These cases protect cross-module identity and display behavior.

use solveig_base::FxHashMap;

use solveig_ast::ast::{
    ConstructorDef, CtorInfo, DataDefinition, Names, ResolvedModule, TyConDefinition, TyConId,
    TyConInfo,
};
use solveig_syntax::lowering::parse_source;

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
    let ctor_ty = Ty::make_arr(Ty::TyUnit, widget_ty);

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
        tycon_kinds: FxHashMap::from_iter([(tycon_id, Kind::Star)]),
        constructor_types: FxHashMap::from_iter([(ctor_id, ctor_ty.clone())]),
        func_schemes: FxHashMap::default(),
        func_defs: Vec::new(),
    };

    let api = build_typed_module_api(
        &session,
        &names,
        module_id,
        &module_name,
        &checked,
        FxHashMap::default(),
        FxHashMap::default(),
    );
    let type_def = super::local_def_id(module_id, session.intern("Widget"));
    let ctor_def = super::local_def_id(module_id, session.intern("MkWidget"));

    assert_eq!(api.type_ids.get(&type_def), Some(&tycon_id));
    assert_eq!(api.constructor_ids.get(&ctor_def), Some(&ctor_id));
    assert_eq!(api.type_name(&names, type_def).as_deref(), Some(type_name));

    let mut tycon_kinds = FxHashMap::default();
    let mut constructor_types = FxHashMap::default();
    let mut func_schemes = FxHashMap::default();
    let mut class_defs = FxHashMap::default();
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

#[test]
fn module_api_filtered_imports_retain_docs() {
    let session = Session::new();
    let source = r#"-- | Adds one to an integer.
documentedIncrement : Int -> Int
documentedIncrement n = n + 1

trailingValue : Int
trailingValue = 7 -- ^ Comes from trailing docs.

-- | Holds a documented integer.
data Documented =
    -- | Wraps a documented integer.
    MkDocumented Int

-- | Describes values.
class Describe a where
    -- | Converts a value into a friendly label.
    describe : a -> String
"#;
    let file = session.add_file("<test>", source);
    let parsed = parse_source(&session, file, source);
    let module_id = ModuleId(1);
    let api = build_module_api(module_id, &parsed).expect("ICE: test API must build");
    let documented_increment = session.intern("documentedIncrement");
    let trailing_value = session.intern("trailingValue");
    let documented = session.intern("Documented");
    let mk_documented = session.intern("MkDocumented");
    let describe_class = session.intern("Describe");
    let describe_method = session.intern("describe");

    let filtered = api.filtered(&ImportSpec::Only(vec![
        documented_increment,
        trailing_value,
        documented,
        mk_documented,
        describe_class,
    ]));

    assert_eq!(
        filtered
            .docs
            .get(&local_def_id(module_id, documented_increment))
            .map(|docs| docs.text.as_str()),
        Some("Adds one to an integer.")
    );
    assert_eq!(
        filtered
            .docs
            .get(&local_def_id(module_id, trailing_value))
            .map(|docs| docs.text.as_str()),
        Some("Comes from trailing docs.")
    );
    assert_eq!(
        filtered
            .docs
            .get(&local_def_id(module_id, documented))
            .map(|docs| docs.text.as_str()),
        Some("Holds a documented integer.")
    );
    assert_eq!(
        filtered
            .docs
            .get(&local_def_id(module_id, describe_class))
            .map(|docs| docs.text.as_str()),
        Some("Describes values.")
    );
    assert_eq!(
        filtered
            .docs
            .get(&local_def_id(module_id, mk_documented))
            .map(|docs| docs.text.as_str()),
        Some("Wraps a documented integer.")
    );
    assert_eq!(
        filtered
            .docs
            .get(&local_def_id(module_id, describe_method))
            .map(|docs| docs.text.as_str()),
        Some("Converts a value into a friendly label.")
    );
}
