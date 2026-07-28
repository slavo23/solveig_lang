use super::*;
use solveig_base::{Session, Symbol};
use std::collections::HashMap;

#[test]
fn identity_tables_allocate_dense_stable_ids() {
    let session = Session::new();

    let mut tycons = TyConTable::new();
    let bool_id = tycons.insert(TyConInfo {
        name: session.intern("Bool"),
        short_name: session.intern("Bool"),
        kind: Kind::Star,
        definition: TyConDefinition::Primitive,
    });
    let list_id = tycons.insert(TyConInfo {
        name: session.intern("List"),
        short_name: session.intern("List"),
        kind: Kind::make_arrow(Kind::Star, Kind::Star),
        definition: TyConDefinition::Primitive,
    });

    assert_eq!(bool_id, TyConId(0));
    assert_eq!(list_id, TyConId(1));
    assert_eq!(
        &*session.str(
            tycons
                .get(bool_id)
                .expect("ICE: inserted Bool type constructor must be present")
                .name
        ),
        "Bool"
    );

    let mut ctors = CtorTable::new();
    let false_id = ctors.insert(CtorInfo {
        name: session.intern("False"),
        short_name: session.intern("False"),
        tycon: bool_id,
        fields: Vec::new(),
        full_type: Ty::make_con_id(bool_id),
        tag: 0,
    });
    let true_id = ctors.insert(CtorInfo {
        name: session.intern("True"),
        short_name: session.intern("True"),
        tycon: bool_id,
        fields: Vec::new(),
        full_type: Ty::make_con_id(bool_id),
        tag: 1,
    });

    assert_eq!(false_id, CtorId(0));
    assert_eq!(true_id, CtorId(1));
    assert_eq!(
        ctors
            .get(true_id)
            .expect("ICE: inserted True constructor must be present")
            .tag,
        1
    );

    let mut classes = ClassTable::new();
    let eq_id = classes.insert(ClassInfo {
        name: session.intern("Eq"),
        short_name: session.intern("Eq"),
        definition: ClassDefinition {
            id: ClassId(u32::MAX),
            name: "Eq".to_string(),
            short_name: "Eq".to_string(),
            params: vec![(TyVarId(0), Kind::Star)],
            superclasses: Vec::new(),
            methods: Vec::new(),
        },
    });

    assert_eq!(eq_id, ClassId(0));
    assert_eq!(
        classes
            .get(eq_id)
            .expect("ICE: inserted Eq class must be present")
            .definition
            .id,
        eq_id
    );
    assert_eq!(
        classes.iter().map(|(id, _)| id).collect::<Vec<_>>(),
        vec![eq_id]
    );
}

#[test]
fn evar_display_round_trips_ids() {
    let id = EvarId(42);

    assert_eq!(evar_display_name(id), "a^42");
    assert_eq!(Ty::make_evar(id), Ty::TyExVar(id));
}

#[test]
fn occurs_check_only_matches_existential_ids() {
    let needle = EvarId(2);

    assert!(Ty::make_evar(needle).occurs_check(needle));
    assert!(!Ty::make_evar(EvarId(3)).occurs_check(needle));
    assert!(!Ty::TyVar(TyVarId(2)).occurs_check(needle));
}

#[test]
fn generated_tyvar_names_generated_ids() {
    let id = TyVarId(9);
    let mut names = Names::new();
    let (fresh_id, fresh_name) = names.fresh_generated_tyvar("t");

    assert_eq!(generated_tyvar_name("t", id), "t9");
    assert_eq!(fresh_id, TyVarId(0));
    assert_eq!(fresh_name, "t0");
    assert_eq!(names.tyvar_display_name(fresh_id), "t0");
}

#[test]
fn names_table_preserves_type_constructor_names() {
    let mut names = Names::new();
    let name = "test.module.Widget";
    let id = names.tycon_id(name);

    assert_eq!(names.tycon_id(name), id);
    assert_ne!(names.tycon_id("test.module.OtherWidget"), id);
    assert_eq!(names.tycon_display_name(id), name);
    assert_eq!(
        Ty::make_named_con(&mut names, name.to_string()),
        Ty::TyCon(id)
    );
    assert_eq!(
        Ty::make_named_con(&mut names, name.to_string()).pretty(),
        format!("T{}", id.0)
    );
}

#[test]
fn names_table_preserves_constructor_names() {
    let mut names = Names::new();
    let name = "test.module.MkWidget";
    let id = names.ctor_id(name);

    assert_eq!(names.ctor_id(name), id);
    assert_ne!(names.ctor_id("test.module.OtherCtor"), id);
    assert_eq!(names.ctor_display_name(id), name);
}

#[test]
fn names_table_preserves_class_names() {
    let mut names = Names::new();
    let name = "test.module.EqWidget";
    let id = names.class_id(name);

    assert_eq!(names.class_id(name), id);
    assert_ne!(names.class_id("test.module.OtherClass"), id);
    assert_eq!(names.class_display_name(id), name);
    assert_eq!(
        Predicate::named(&mut names, name.to_string(), Vec::new()).class_id,
        id
    );
    assert_eq!(
        Predicate::named(&mut names, name.to_string(), Vec::new()).pretty(),
        format!("K{}", id.0)
    );
}

#[test]
fn resolved_module_rebuilds_identity_tables_for_named_ids() {
    let mut names = Names::new();
    let tycon_name = "test.module.Widget";
    let ctor_name = "test.module.MkWidget";
    let class_name = "test.module.WidgetLike";
    let tycon_id = names.tycon_id(tycon_name);
    let ctor_id = names.ctor_id(ctor_name);
    let class_id = names.class_id(class_name);

    let data_def = DataDefinition {
        name: tycon_name.to_string(),
        short_name: "Widget".to_string(),
        ty_params: Vec::new(),
        tycon_kind: Kind::Star,
        constructors: vec![ConstructorDef {
            id: ctor_id,
            name: ctor_name.to_string(),
            short_name: "MkWidget".to_string(),
            fields: vec![Ty::TyUnit],
            full_type: Ty::make_arr(
                Ty::TyUnit,
                Ty::make_named_con(&mut names, tycon_name.to_string()),
            ),
        }],
    };

    let class_def = ClassDefinition {
        id: class_id,
        name: class_name.to_string(),
        short_name: "WidgetLike".to_string(),
        params: Vec::new(),
        superclasses: Vec::new(),
        methods: Vec::new(),
    };

    let mut module = ResolvedModule::<()> {
        data_defs: vec![data_def],
        class_defs: vec![class_def],
        instance_defs: Vec::new(),
        builtin_defs: Vec::new(),
        tycon_table: TyConTable::new(),
        ctor_table: CtorTable::new(),
        class_table: ClassTable::new(),
        tycon_kinds: HashMap::new(),
        constructor_types: HashMap::new(),
        func_schemes: HashMap::new(),
        func_defs: Vec::new(),
    };

    module.rebuild_identity_tables(&mut names);

    let tycon_info = module
        .tycon_table
        .get(tycon_id)
        .expect("ICE: rebuilt type-constructor table must contain Widget");
    assert_eq!(names.symbol_display_name(tycon_info.name), tycon_name);
    assert_eq!(tycon_info.kind, Kind::Star);

    let ctor_info = module
        .ctor_table
        .get(ctor_id)
        .expect("ICE: rebuilt constructor table must contain MkWidget");
    assert_eq!(names.symbol_display_name(ctor_info.name), ctor_name);
    assert_eq!(ctor_info.tycon, tycon_id);
    assert_eq!(ctor_info.tag, 0);

    let class_info = module
        .class_table
        .get(class_id)
        .expect("ICE: rebuilt class table must contain WidgetLike");
    assert_eq!(names.symbol_display_name(class_info.name), class_name);
    assert_eq!(class_info.definition.id, class_id);
}

#[test]
fn table_backed_pretty_uses_identity_tables() {
    let tycon_id = TyConId(42);
    let class_id = ClassId(77);
    let mut names = Names::new();
    let mut tycons = TyConTable::new();
    let mut classes = ClassTable::new();

    tycons.insert_with_id(
        tycon_id,
        TyConInfo {
            name: names.symbol("test.module.TableTy"),
            short_name: names.symbol("TableTy"),
            kind: Kind::Star,
            definition: TyConDefinition::Primitive,
        },
    );
    classes.insert_with_id(
        class_id,
        ClassInfo {
            name: names.symbol("test.module.TableClass"),
            short_name: names.symbol("TableClass"),
            definition: ClassDefinition {
                id: class_id,
                name: "test.module.TableClass".to_string(),
                short_name: "TableClass".to_string(),
                params: Vec::new(),
                superclasses: Vec::new(),
                methods: Vec::new(),
            },
        },
    );

    let tables = PrettyTables {
        names: &names,
        tycons: &tycons,
        classes: &classes,
    };
    let scheme = Scheme {
        quantifiers: Vec::new(),
        predicates: vec![Predicate {
            class_id,
            args: vec![Ty::TyCon(tycon_id)],
        }],
        ty: Ty::TyCon(tycon_id),
    };

    assert_eq!(Ty::TyCon(tycon_id).pretty(), "T42");
    assert_eq!(
        scheme.pretty_with_tables(tables),
        "TableClass TableTy => TableTy"
    );
}

#[test]
fn names_table_preserves_local_and_global_names() {
    let mut names = Names::new();
    let local = VarRef::named_local(&mut names, "value");
    let global_name = "test.module.value";
    let global = VarRef::named_global(&mut names, global_name);

    assert_eq!(names.var_display_name(&local), "value");
    assert_eq!(names.var_display_name(&global), global_name);

    let source_symbol = Symbol(12);
    names.register_symbol(source_symbol, "sourceValue");
    assert_eq!(
        names.var_display_name(&VarRef::Local(source_symbol)),
        "sourceValue"
    );

    let def_id = DefId {
        module: ModuleId(7),
        name: Symbol(11),
    };
    names.register_def(def_id, "other.module.value");

    assert_eq!(
        names.var_display_name(&VarRef::Global(def_id)),
        "other.module.value"
    );
}
