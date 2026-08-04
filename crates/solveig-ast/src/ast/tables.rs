//! Core identity tables for data constructors, type constructors, and classes.
//! These tables bind stable IDs to the definitions retained by later compiler stages.

use solveig_base::FxHashMap;
use std::ops::Range;

use solveig_base::Symbol;

use super::*;

pub struct PrimTys {
    pub int: TyConId,
    pub float: TyConId,
    pub char: TyConId,
    pub string: TyConId,
    pub io: TyConId,
    pub bool: TyConId,
    pub true_ctor: CtorId,
    pub false_ctor: CtorId,
}

pub const PRIM_TYS: PrimTys = PrimTys {
    int: TyConId(0),
    float: TyConId(1),
    char: TyConId(2),
    string: TyConId(3),
    io: TyConId(4),
    bool: TyConId(5),
    true_ctor: CtorId(0),
    false_ctor: CtorId(1),
};

impl PrimTys {
    pub fn open_primitive_tycon_ids(self) -> [TyConId; 4] {
        [self.int, self.float, self.char, self.string]
    }
}

pub fn compiler_primitive_tycon_infos(names: &mut Names) -> Vec<(TyConId, TyConInfo)> {
    vec![
        compiler_primitive_tycon_info(names, PRIM_TYS.int, COMPILER_INT_NAME, "Int", Kind::Star),
        compiler_primitive_tycon_info(
            names,
            PRIM_TYS.float,
            COMPILER_FLOAT_NAME,
            "Float",
            Kind::Star,
        ),
        compiler_primitive_tycon_info(names, PRIM_TYS.char, COMPILER_CHAR_NAME, "Char", Kind::Star),
        compiler_primitive_tycon_info(
            names,
            PRIM_TYS.string,
            COMPILER_STRING_NAME,
            "String",
            Kind::Star,
        ),
        compiler_primitive_tycon_info(
            names,
            PRIM_TYS.io,
            COMPILER_IO_NAME,
            "IO",
            Kind::make_arrow(Kind::Star, Kind::Star),
        ),
    ]
}

pub fn compiler_primitive_tycon_kinds() -> FxHashMap<TyConId, Kind> {
    FxHashMap::from_iter([
        (PRIM_TYS.int, Kind::Star),
        (PRIM_TYS.float, Kind::Star),
        (PRIM_TYS.char, Kind::Star),
        (PRIM_TYS.string, Kind::Star),
        (PRIM_TYS.io, Kind::make_arrow(Kind::Star, Kind::Star)),
    ])
}

pub fn compiler_bool_data_definition() -> DataDefinition {
    let bool_ty = Ty::make_con_id(PRIM_TYS.bool);

    DataDefinition {
        name: COMPILER_BOOL_NAME.to_string(),
        short_name: "Bool".to_string(),
        ty_params: Vec::new(),
        tycon_kind: Kind::Star,
        constructors: vec![
            ConstructorDef {
                id: PRIM_TYS.true_ctor,
                name: COMPILER_TRUE_NAME.to_string(),
                short_name: "True".to_string(),
                fields: Vec::new(),
                full_type: bool_ty.clone(),
            },
            ConstructorDef {
                id: PRIM_TYS.false_ctor,
                name: COMPILER_FALSE_NAME.to_string(),
                short_name: "False".to_string(),
                fields: Vec::new(),
                full_type: bool_ty,
            },
        ],
    }
}

pub fn compiler_bool_constructor_types() -> FxHashMap<CtorId, Ty> {
    compiler_bool_data_definition()
        .constructors
        .into_iter()
        .map(|ctor| (ctor.id, ctor.full_type))
        .collect()
}

pub fn prelude_maybe_tycon_id(names: &mut Names) -> TyConId {
    names.tycon_id(PRELUDE_MAYBE_NAME)
}

pub fn prelude_maybe_just_ctor_id(names: &mut Names) -> CtorId {
    names.ctor_id(PRELUDE_MAYBE_JUST_NAME)
}

pub fn prelude_maybe_nothing_ctor_id(names: &mut Names) -> CtorId {
    names.ctor_id(PRELUDE_MAYBE_NOTHING_NAME)
}

pub fn prelude_list_tycon_id(names: &mut Names) -> TyConId {
    names.tycon_id(PRELUDE_LIST_NAME)
}

pub fn prelude_list_nil_ctor_id(names: &mut Names) -> CtorId {
    names.ctor_id(PRELUDE_LIST_NIL_NAME)
}

pub fn prelude_list_cons_ctor_id(names: &mut Names) -> CtorId {
    names.ctor_id(PRELUDE_LIST_CONS_NAME)
}

pub fn prelude_pair_mkpair_ctor_id(names: &mut Names) -> CtorId {
    names.ctor_id(PRELUDE_PAIR_MKPAIR_NAME)
}

pub fn prelude_ordering_tycon_id(names: &mut Names) -> TyConId {
    names.tycon_id(PRELUDE_ORDERING_NAME)
}

pub fn prelude_ordering_lt_ctor_id(names: &mut Names) -> CtorId {
    names.ctor_id(PRELUDE_ORDERING_LT_NAME)
}

pub fn prelude_ordering_eq_ctor_id(names: &mut Names) -> CtorId {
    names.ctor_id(PRELUDE_ORDERING_EQ_NAME)
}

pub fn prelude_ordering_gt_ctor_id(names: &mut Names) -> CtorId {
    names.ctor_id(PRELUDE_ORDERING_GT_NAME)
}

pub fn runtime_ctor_role(names: &Names, ctor: CtorId) -> CtorRuntimeRole {
    match names.ctor_display_name(ctor).as_str() {
        PRELUDE_MAYBE_JUST_NAME => CtorRuntimeRole::PreludeMaybeJust,
        PRELUDE_MAYBE_NOTHING_NAME => CtorRuntimeRole::PreludeMaybeNothing,
        PRELUDE_LIST_NIL_NAME => CtorRuntimeRole::PreludeListNil,
        PRELUDE_LIST_CONS_NAME => CtorRuntimeRole::PreludeListCons,
        PRELUDE_PAIR_MKPAIR_NAME => CtorRuntimeRole::PreludePair,
        PRELUDE_ORDERING_LT_NAME => CtorRuntimeRole::PreludeOrderingLt,
        PRELUDE_ORDERING_EQ_NAME => CtorRuntimeRole::PreludeOrderingEq,
        PRELUDE_ORDERING_GT_NAME => CtorRuntimeRole::PreludeOrderingGt,
        _ => CtorRuntimeRole::Ordinary,
    }
}

fn compiler_primitive_tycon_info(
    names: &mut Names,
    id: TyConId,
    name: &str,
    short_name: &str,
    kind: Kind,
) -> (TyConId, TyConInfo) {
    (
        id,
        TyConInfo {
            name: names.symbol(name),
            short_name: names.symbol(short_name),
            kind,
            definition: TyConDefinition::Primitive,
        },
    )
}

fn tycon_id_for_data_definition(names: &mut Names, name: &str) -> TyConId {
    if name == COMPILER_BOOL_NAME {
        PRIM_TYS.bool
    } else {
        names.tycon_id(name)
    }
}

pub fn identity_tables_from_defs(
    names: &mut Names,
    data_defs: &[DataDefinition],
    class_defs: &[ClassDefinition],
) -> (TyConTable, CtorTable, ClassTable) {
    let mut tycon_table = TyConTable::new();
    let mut ctor_table = CtorTable::new();
    let mut class_table = ClassTable::new();
    let mut next_ctor_tag = 0_u16;

    for (id, info) in compiler_primitive_tycon_infos(names) {
        tycon_table.insert_with_id(id, info);
    }

    if let Some(bool_def) = data_defs
        .iter()
        .find(|data_def| data_def.name == COMPILER_BOOL_NAME)
    {
        insert_data_definition_identity(
            names,
            bool_def,
            &mut tycon_table,
            &mut ctor_table,
            &mut next_ctor_tag,
        );
    }

    for data_def in data_defs {
        if data_def.name == COMPILER_BOOL_NAME {
            continue;
        }

        insert_data_definition_identity(
            names,
            data_def,
            &mut tycon_table,
            &mut ctor_table,
            &mut next_ctor_tag,
        );
    }

    for class_def in class_defs {
        class_table.insert_with_id(
            class_def.id,
            ClassInfo {
                name: names.symbol(&class_def.name),
                short_name: names.symbol(&class_def.short_name),
                definition: class_def.clone(),
            },
        );
    }

    (tycon_table, ctor_table, class_table)
}

fn insert_data_definition_identity(
    names: &mut Names,
    data_def: &DataDefinition,
    tycon_table: &mut TyConTable,
    ctor_table: &mut CtorTable,
    next_ctor_tag: &mut u16,
) {
    let tycon_id = tycon_id_for_data_definition(names, &data_def.name);

    tycon_table.insert_with_id(
        tycon_id,
        TyConInfo {
            name: names.symbol(&data_def.name),
            short_name: names.symbol(&data_def.short_name),
            kind: data_def.tycon_kind.clone(),
            definition: TyConDefinition::Data(data_def.clone()),
        },
    );

    for ctor_def in &data_def.constructors {
        ctor_table.insert_with_id(
            ctor_def.id,
            CtorInfo {
                name: names.symbol(&ctor_def.name),
                short_name: names.symbol(&ctor_def.short_name),
                tycon: tycon_id,
                fields: ctor_def.fields.clone(),
                full_type: ctor_def.full_type.clone(),
                tag: *next_ctor_tag,
            },
        );

        *next_ctor_tag = (*next_ctor_tag)
            .checked_add(1)
            .expect("ICE: constructor tag overflow while rebuilding identity tables");
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataDefinition {
    pub name: String,
    pub short_name: String,
    pub ty_params: Vec<(TyVarId, Kind)>,
    pub tycon_kind: Kind,
    pub constructors: Vec<ConstructorDef>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TyConDefinition {
    Data(DataDefinition),
    Primitive,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TyConInfo {
    pub name: Symbol,
    pub short_name: Symbol,
    pub kind: Kind,
    pub definition: TyConDefinition,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TyConTable {
    entries: Vec<(TyConId, TyConInfo)>,
}

impl TyConTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, info: TyConInfo) -> TyConId {
        let id = TyConId(
            u32::try_from(self.entries.len())
                .expect("ICE: type constructor table exceeded u32 capacity"),
        );
        self.insert_with_id(id, info);
        id
    }

    pub fn insert_with_id(&mut self, id: TyConId, info: TyConInfo) {
        if let Some((_, existing)) = self
            .entries
            .iter_mut()
            .find(|(existing, _)| *existing == id)
        {
            *existing = info;
            return;
        }

        self.entries.push((id, info));
    }

    pub fn get(&self, id: TyConId) -> Option<&TyConInfo> {
        self.entries
            .iter()
            .find(|(existing, _)| *existing == id)
            .map(|(_, info)| info)
    }

    pub fn iter(&self) -> impl Iterator<Item = (TyConId, &TyConInfo)> {
        self.entries.iter().map(|(id, info)| (*id, info))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConstructorDef {
    pub id: CtorId,
    pub name: String,
    pub short_name: String,
    pub fields: Vec<Ty>,
    pub full_type: Ty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtorRuntimeInfo {
    pub ctor: CtorId,
    pub role: CtorRuntimeRole,
    pub name: String,
    pub short_name: String,
    pub tag: u16,
    pub arity: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtorRuntimeRole {
    Ordinary,
    PreludeMaybeJust,
    PreludeMaybeNothing,
    PreludeListNil,
    PreludeListCons,
    PreludePair,
    PreludeOrderingLt,
    PreludeOrderingEq,
    PreludeOrderingGt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CtorInfo {
    pub name: Symbol,
    pub short_name: Symbol,
    pub tycon: TyConId,
    pub fields: Vec<Ty>,
    pub full_type: Ty,
    pub tag: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CtorTable {
    entries: Vec<(CtorId, CtorInfo)>,
}

impl CtorTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, info: CtorInfo) -> CtorId {
        let id = CtorId(
            u32::try_from(self.entries.len())
                .expect("ICE: constructor table exceeded u32 capacity"),
        );
        self.insert_with_id(id, info);
        id
    }

    pub fn insert_with_id(&mut self, id: CtorId, info: CtorInfo) {
        if let Some((_, existing)) = self
            .entries
            .iter_mut()
            .find(|(existing, _)| *existing == id)
        {
            *existing = info;
            return;
        }

        self.entries.push((id, info));
    }

    pub fn get(&self, id: CtorId) -> Option<&CtorInfo> {
        self.entries
            .iter()
            .find(|(existing, _)| *existing == id)
            .map(|(_, info)| info)
    }

    pub fn iter(&self) -> impl Iterator<Item = (CtorId, &CtorInfo)> {
        self.entries.iter().map(|(id, info)| (*id, info))
    }
}

#[derive(Clone, Debug)]
pub struct ClassMethodDef {
    pub id: DefId,
    pub name: String,
    pub short_name: String,
    pub scheme: Scheme,
    pub default_body: Option<TermExprNode<Range<usize>>>,
}

#[derive(Clone, Debug)]
pub struct ClassDefinition {
    pub id: ClassId,
    pub name: String,
    pub short_name: String,
    pub params: Vec<(TyVarId, Kind)>,
    pub superclasses: Vec<Predicate>,
    pub methods: Vec<ClassMethodDef>,
}

#[derive(Clone, Debug)]
pub struct ClassInfo {
    pub name: Symbol,
    pub short_name: Symbol,
    pub definition: ClassDefinition,
}

#[derive(Clone, Debug, Default)]
pub struct ClassTable {
    entries: Vec<(ClassId, ClassInfo)>,
}

impl ClassTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, mut info: ClassInfo) -> ClassId {
        let id = ClassId(
            u32::try_from(self.entries.len()).expect("ICE: class table exceeded u32 capacity"),
        );
        info.definition.id = id;
        self.insert_with_id(id, info);
        id
    }

    pub fn insert_with_id(&mut self, id: ClassId, mut info: ClassInfo) {
        info.definition.id = id;

        if let Some((_, existing)) = self
            .entries
            .iter_mut()
            .find(|(existing, _)| *existing == id)
        {
            *existing = info;
            return;
        }

        self.entries.push((id, info));
    }

    pub fn get(&self, id: ClassId) -> Option<&ClassInfo> {
        self.entries
            .iter()
            .find(|(existing, _)| *existing == id)
            .map(|(_, info)| info)
    }

    pub fn iter(&self) -> impl Iterator<Item = (ClassId, &ClassInfo)> {
        self.entries.iter().map(|(id, info)| (*id, info))
    }
}
