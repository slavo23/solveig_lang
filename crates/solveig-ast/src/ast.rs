use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Mutex, OnceLock};

use solveig_base::Symbol;

const PRECEDENCE_LOOSE: u8 = 0;
const PRECEDENCE_ARR: u8 = 1;
const PRECEDENCE_APP: u8 = 2;
const PRECEDENCE_ATOM: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefId {
    pub module: ModuleId,
    pub name: Symbol,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TyVarId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EvarId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TyConId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CtorId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClassId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InstanceId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EvVarId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BuiltinId(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StrLitId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KindMetaId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Ev {
    Var(EvVarId),
    Placeholder(EvVarId),
    Inst(InstanceId, Vec<TypeExpr>, Vec<Ev>),
    Super(Box<Ev>, ClassId, usize),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum VarRef {
    Local(Symbol),
    Global(DefId),
}

pub trait Pretty<T> {
    fn pretty(&self) -> String;
    fn get_precedence(t: &T) -> u8;
}

#[derive(Clone, PartialEq, Debug, Eq, Hash)]
pub enum Kind {
    Meta(KindMetaId),
    Star,
    Arrow(Box<Kind>, Box<Kind>),
}

impl Kind {
    pub fn make_star() -> Self {
        Kind::Star
    }

    pub fn make_meta(id: KindMetaId) -> Self {
        Kind::Meta(id)
    }

    pub fn make_arrow(lhs: Kind, rhs: Kind) -> Self {
        Kind::Arrow(Box::new(lhs), Box::new(rhs))
    }
}

impl Pretty<Kind> for Kind {
    fn get_precedence(kind: &Kind) -> u8 {
        match kind {
            Kind::Star | Kind::Meta(_) => PRECEDENCE_ATOM,
            Kind::Arrow(_, _) => PRECEDENCE_ARR,
        }
    }

    fn pretty(&self) -> String {
        fn traverse_helper(kind: &Kind, parent_precedence: u8) -> String {
            let curr_precedence = Kind::get_precedence(kind);
            let kind_fmt = match kind {
                Kind::Star => "*".to_string(),
                Kind::Meta(id) => format!("?k{}", id.0),
                Kind::Arrow(lhs, rhs) => {
                    let lhs_fmt = traverse_helper(lhs, PRECEDENCE_ATOM);
                    let rhs_fmt = traverse_helper(rhs, PRECEDENCE_ARR);
                    format!("{lhs_fmt} -> {rhs_fmt}")
                }
            };

            if curr_precedence < parent_precedence {
                format!("({kind_fmt})")
            } else {
                kind_fmt
            }
        }

        traverse_helper(self, PRECEDENCE_ARR)
    }
}

#[derive(Clone, PartialEq, Debug, Eq, Hash)]
pub enum Ty {
    TyUnit,
    TyVar(TyVarId),
    TyExVar(EvarId),
    TyArr(Box<Ty>, Box<Ty>),
    TyForAll(Vec<(TyVarId, Kind)>, Vec<Predicate>, Box<Ty>),
    TyCon(TyConId),
    TyApp(Box<Ty>, Box<Ty>),
}

pub type TypeExpr = Ty;

pub fn legacy_evar_name(id: EvarId) -> String {
    format!("a^{}", id.0)
}

const LEGACY_TYCON_TAG: u32 = 0x8000_0000;
const LEGACY_CTOR_TAG: u32 = 0x8000_0000;
const LEGACY_CLASS_TAG: u32 = 0x8000_0000;
const LEGACY_SYMBOL_TAG: u32 = 0x8000_0000;
const LEGACY_DEF_MODULE: ModuleId = ModuleId(0xffff_fffe);

#[derive(Default)]
struct LegacyTyConRegistry {
    names: Vec<String>,
    ids: HashMap<String, TyConId>,
}

#[derive(Default)]
struct LegacyCtorRegistry {
    names: Vec<String>,
    ids: HashMap<String, CtorId>,
}

#[derive(Default)]
struct LegacyClassRegistry {
    names: Vec<String>,
    ids: HashMap<String, ClassId>,
}

#[derive(Default)]
struct LegacyTermNameRegistry {
    symbols: Vec<String>,
    symbol_ids: HashMap<String, Symbol>,
    symbol_names: HashMap<Symbol, String>,
    def_names: HashMap<DefId, String>,
}

#[derive(Default)]
struct StringLiteralRegistry {
    values: Vec<String>,
    ids: HashMap<String, StrLitId>,
}

fn legacy_tycon_registry() -> &'static Mutex<LegacyTyConRegistry> {
    static REGISTRY: OnceLock<Mutex<LegacyTyConRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(LegacyTyConRegistry::default()))
}

fn legacy_ctor_registry() -> &'static Mutex<LegacyCtorRegistry> {
    static REGISTRY: OnceLock<Mutex<LegacyCtorRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(LegacyCtorRegistry::default()))
}

fn legacy_class_registry() -> &'static Mutex<LegacyClassRegistry> {
    static REGISTRY: OnceLock<Mutex<LegacyClassRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(LegacyClassRegistry::default()))
}

fn legacy_term_name_registry() -> &'static Mutex<LegacyTermNameRegistry> {
    static REGISTRY: OnceLock<Mutex<LegacyTermNameRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(LegacyTermNameRegistry::default()))
}

fn string_literal_registry() -> &'static Mutex<StringLiteralRegistry> {
    static REGISTRY: OnceLock<Mutex<StringLiteralRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(StringLiteralRegistry::default()))
}

pub fn legacy_tycon_id(name: &str) -> TyConId {
    let mut registry = legacy_tycon_registry()
        .lock()
        .expect("ICE: legacy type constructor registry lock poisoned");

    if let Some(id) = registry.ids.get(name) {
        return *id;
    }

    let id = TyConId(LEGACY_TYCON_TAG | registry.names.len() as u32);
    registry.names.push(name.to_string());
    registry.ids.insert(name.to_string(), id);
    id
}

pub fn tycon_display_name(id: TyConId) -> String {
    let registry = legacy_tycon_registry()
        .lock()
        .expect("ICE: legacy type constructor registry lock poisoned");
    let index = (id.0 & !LEGACY_TYCON_TAG) as usize;

    if id.0 & LEGACY_TYCON_TAG != 0
        && let Some(name) = registry.names.get(index)
    {
        return name.clone();
    }

    tycon_fallback_name(id)
}

pub fn legacy_ctor_id(name: &str) -> CtorId {
    let mut registry = legacy_ctor_registry()
        .lock()
        .expect("ICE: legacy constructor registry lock poisoned");

    if let Some(id) = registry.ids.get(name) {
        return *id;
    }

    let id = CtorId(LEGACY_CTOR_TAG | registry.names.len() as u32);
    registry.names.push(name.to_string());
    registry.ids.insert(name.to_string(), id);
    id
}

pub fn ctor_display_name(id: CtorId) -> String {
    let registry = legacy_ctor_registry()
        .lock()
        .expect("ICE: legacy constructor registry lock poisoned");
    let index = (id.0 & !LEGACY_CTOR_TAG) as usize;

    if id.0 & LEGACY_CTOR_TAG != 0
        && let Some(name) = registry.names.get(index)
    {
        return name.clone();
    }

    ctor_fallback_name(id)
}

pub fn legacy_class_id(name: &str) -> ClassId {
    let mut registry = legacy_class_registry()
        .lock()
        .expect("ICE: legacy class registry lock poisoned");

    if let Some(id) = registry.ids.get(name) {
        return *id;
    }

    let id = ClassId(LEGACY_CLASS_TAG | registry.names.len() as u32);
    registry.names.push(name.to_string());
    registry.ids.insert(name.to_string(), id);
    id
}

pub fn class_display_name(id: ClassId) -> String {
    let registry = legacy_class_registry()
        .lock()
        .expect("ICE: legacy class registry lock poisoned");
    let index = (id.0 & !LEGACY_CLASS_TAG) as usize;

    if id.0 & LEGACY_CLASS_TAG != 0
        && let Some(name) = registry.names.get(index)
    {
        return name.clone();
    }

    class_fallback_name(id)
}

fn tycon_fallback_name(id: TyConId) -> String {
    format!("T{}", id.0)
}

fn ctor_fallback_name(id: CtorId) -> String {
    format!("C{}", id.0)
}

fn class_fallback_name(id: ClassId) -> String {
    format!("K{}", id.0)
}

pub fn legacy_symbol_id(name: &str) -> Symbol {
    let mut registry = legacy_term_name_registry()
        .lock()
        .expect("ICE: legacy term-name registry lock poisoned");

    if let Some(id) = registry.symbol_ids.get(name) {
        return *id;
    }

    let id = Symbol(LEGACY_SYMBOL_TAG | registry.symbols.len() as u32);
    registry.symbols.push(name.to_string());
    registry.symbol_ids.insert(name.to_string(), id);
    registry.symbol_names.insert(id, name.to_string());
    id
}

pub fn intern_string_literal(value: &str) -> StrLitId {
    let mut registry = string_literal_registry()
        .lock()
        .expect("ICE: string-literal registry lock poisoned");

    if let Some(id) = registry.ids.get(value) {
        return *id;
    }

    let id = StrLitId(registry.values.len() as u32);
    registry.values.push(value.to_string());
    registry.ids.insert(value.to_string(), id);
    id
}

pub fn string_literal_value(id: StrLitId) -> String {
    let registry = string_literal_registry()
        .lock()
        .expect("ICE: string-literal registry lock poisoned");

    registry
        .values
        .get(id.0 as usize)
        .cloned()
        .unwrap_or_else(|| panic!("ICE: missing string literal payload for StrLitId {id:?}"))
}

pub const COMPILER_PRELUDE_BUILTINS_MODULE: &str = "prelude.builtins";
pub const COMPILER_PRELUDE_PRIM_MODULE: &str = "prelude.prim";
pub const COMPILER_INT_NAME: &str = "prelude.builtins.Int";
pub const COMPILER_FLOAT_NAME: &str = "prelude.builtins.Float";
pub const COMPILER_CHAR_NAME: &str = "prelude.builtins.Char";
pub const COMPILER_STRING_NAME: &str = "prelude.builtins.String";
pub const COMPILER_IO_NAME: &str = "prelude.builtins.IO";
pub const COMPILER_BOOL_NAME: &str = "prelude.builtins.Bool";
pub const COMPILER_TRUE_NAME: &str = "prelude.builtins.True";
pub const COMPILER_FALSE_NAME: &str = "prelude.builtins.False";
pub const PRELUDE_MAYBE_NAME: &str = "prelude.builtins.Maybe";
pub const PRELUDE_MAYBE_JUST_NAME: &str = "prelude.builtins.Just";
pub const PRELUDE_MAYBE_NOTHING_NAME: &str = "prelude.builtins.Nothing";
pub const PRELUDE_LIST_NAME: &str = "prelude.list.List";
pub const PRELUDE_LIST_NIL_NAME: &str = "prelude.list.Nil";
pub const PRELUDE_LIST_CONS_NAME: &str = "prelude.list.Cons";
pub const PRELUDE_PAIR_MKPAIR_NAME: &str = "prelude.builtins.MkPair";
pub const PRELUDE_ORDERING_NAME: &str = "prelude.typeclasses.Ordering";
pub const PRELUDE_ORDERING_LT_NAME: &str = "prelude.typeclasses.LT";
pub const PRELUDE_ORDERING_EQ_NAME: &str = "prelude.typeclasses.EQ";
pub const PRELUDE_ORDERING_GT_NAME: &str = "prelude.typeclasses.GT";

pub const BUILTIN_INT_ADD: BuiltinId = BuiltinId(0);
pub const BUILTIN_INT_SUB: BuiltinId = BuiltinId(1);
pub const BUILTIN_INT_MUL: BuiltinId = BuiltinId(2);
pub const BUILTIN_INT_DIV: BuiltinId = BuiltinId(3);
pub const BUILTIN_INT_MOD: BuiltinId = BuiltinId(4);
pub const BUILTIN_INT_NEG: BuiltinId = BuiltinId(5);
pub const BUILTIN_INT_EQ: BuiltinId = BuiltinId(6);
pub const BUILTIN_INT_LT: BuiltinId = BuiltinId(7);
pub const BUILTIN_INT_LE: BuiltinId = BuiltinId(8);
pub const BUILTIN_INT_GT: BuiltinId = BuiltinId(9);
pub const BUILTIN_INT_GE: BuiltinId = BuiltinId(10);
pub const BUILTIN_INT_COMPARE: BuiltinId = BuiltinId(11);
pub const BUILTIN_FLOAT_ADD: BuiltinId = BuiltinId(12);
pub const BUILTIN_FLOAT_SUB: BuiltinId = BuiltinId(13);
pub const BUILTIN_FLOAT_MUL: BuiltinId = BuiltinId(14);
pub const BUILTIN_FLOAT_DIV: BuiltinId = BuiltinId(15);
pub const BUILTIN_FLOAT_NEG: BuiltinId = BuiltinId(16);
pub const BUILTIN_FLOAT_EQ: BuiltinId = BuiltinId(17);
pub const BUILTIN_FLOAT_LT: BuiltinId = BuiltinId(18);
pub const BUILTIN_FLOAT_LE: BuiltinId = BuiltinId(19);
pub const BUILTIN_FLOAT_GT: BuiltinId = BuiltinId(20);
pub const BUILTIN_FLOAT_GE: BuiltinId = BuiltinId(21);
pub const BUILTIN_INT_TO_FLOAT: BuiltinId = BuiltinId(22);
pub const BUILTIN_FLOAT_TO_INT: BuiltinId = BuiltinId(23);
pub const BUILTIN_INT_TO_STRING: BuiltinId = BuiltinId(24);
pub const BUILTIN_FLOAT_TO_STRING: BuiltinId = BuiltinId(25);
pub const BUILTIN_CHAR_TO_STRING: BuiltinId = BuiltinId(26);
pub const BUILTIN_STRING_LENGTH: BuiltinId = BuiltinId(27);
pub const BUILTIN_STRING_APPEND: BuiltinId = BuiltinId(28);
pub const BUILTIN_STRING_SLICE: BuiltinId = BuiltinId(29);
pub const BUILTIN_STRING_CHAR_AT: BuiltinId = BuiltinId(30);
pub const BUILTIN_STRING_EQ: BuiltinId = BuiltinId(31);
pub const BUILTIN_STRING_COMPARE: BuiltinId = BuiltinId(32);
pub const BUILTIN_CHAR_EQ: BuiltinId = BuiltinId(33);
pub const BUILTIN_CHAR_COMPARE: BuiltinId = BuiltinId(34);
pub const BUILTIN_STRING_TO_CHARS: BuiltinId = BuiltinId(35);
pub const BUILTIN_CHARS_TO_STRING: BuiltinId = BuiltinId(36);
pub const BUILTIN_PURE_IO: BuiltinId = BuiltinId(37);
pub const BUILTIN_BIND_IO: BuiltinId = BuiltinId(38);
pub const BUILTIN_PRINT_LINE: BuiltinId = BuiltinId(39);
pub const BUILTIN_PRINT_STR: BuiltinId = BuiltinId(40);
pub const BUILTIN_READ_LINE: BuiltinId = BuiltinId(41);
pub const BUILTIN_GET_ARGS: BuiltinId = BuiltinId(42);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

pub fn compiler_primitive_tycon_infos() -> Vec<(TyConId, TyConInfo)> {
    vec![
        compiler_primitive_tycon_info(PRIM_TYS.int, COMPILER_INT_NAME, "Int", Kind::Star),
        compiler_primitive_tycon_info(PRIM_TYS.float, COMPILER_FLOAT_NAME, "Float", Kind::Star),
        compiler_primitive_tycon_info(PRIM_TYS.char, COMPILER_CHAR_NAME, "Char", Kind::Star),
        compiler_primitive_tycon_info(PRIM_TYS.string, COMPILER_STRING_NAME, "String", Kind::Star),
        compiler_primitive_tycon_info(
            PRIM_TYS.io,
            COMPILER_IO_NAME,
            "IO",
            Kind::make_arrow(Kind::Star, Kind::Star),
        ),
    ]
}

pub fn compiler_primitive_tycon_kinds() -> HashMap<TyConId, Kind> {
    compiler_primitive_tycon_infos()
        .into_iter()
        .map(|(id, info)| (id, info.kind))
        .collect()
}

pub fn compiler_bool_data_definition() -> DataDefinition {
    let bool_ty = TypeExpr::make_con_id(PRIM_TYS.bool);

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

pub fn compiler_bool_constructor_types() -> HashMap<CtorId, TypeExpr> {
    compiler_bool_data_definition()
        .constructors
        .into_iter()
        .map(|ctor| (ctor.id, ctor.full_type))
        .collect()
}

pub fn prelude_maybe_tycon_id() -> TyConId {
    legacy_tycon_id(PRELUDE_MAYBE_NAME)
}

pub fn prelude_maybe_just_ctor_id() -> CtorId {
    legacy_ctor_id(PRELUDE_MAYBE_JUST_NAME)
}

pub fn prelude_maybe_nothing_ctor_id() -> CtorId {
    legacy_ctor_id(PRELUDE_MAYBE_NOTHING_NAME)
}

pub fn prelude_list_tycon_id() -> TyConId {
    legacy_tycon_id(PRELUDE_LIST_NAME)
}

pub fn prelude_list_nil_ctor_id() -> CtorId {
    legacy_ctor_id(PRELUDE_LIST_NIL_NAME)
}

pub fn prelude_list_cons_ctor_id() -> CtorId {
    legacy_ctor_id(PRELUDE_LIST_CONS_NAME)
}

pub fn prelude_pair_mkpair_ctor_id() -> CtorId {
    legacy_ctor_id(PRELUDE_PAIR_MKPAIR_NAME)
}

pub fn prelude_ordering_tycon_id() -> TyConId {
    legacy_tycon_id(PRELUDE_ORDERING_NAME)
}

pub fn prelude_ordering_lt_ctor_id() -> CtorId {
    legacy_ctor_id(PRELUDE_ORDERING_LT_NAME)
}

pub fn prelude_ordering_eq_ctor_id() -> CtorId {
    legacy_ctor_id(PRELUDE_ORDERING_EQ_NAME)
}

pub fn prelude_ordering_gt_ctor_id() -> CtorId {
    legacy_ctor_id(PRELUDE_ORDERING_GT_NAME)
}

pub fn runtime_ctor_role(ctor: CtorId) -> CtorRuntimeRole {
    if ctor == prelude_list_nil_ctor_id() {
        CtorRuntimeRole::PreludeListNil
    } else if ctor == prelude_list_cons_ctor_id() {
        CtorRuntimeRole::PreludeListCons
    } else if ctor == prelude_pair_mkpair_ctor_id() {
        CtorRuntimeRole::PreludePair
    } else {
        CtorRuntimeRole::Ordinary
    }
}

fn compiler_primitive_tycon_info(
    id: TyConId,
    name: &str,
    short_name: &str,
    kind: Kind,
) -> (TyConId, TyConInfo) {
    (
        id,
        TyConInfo {
            name: legacy_symbol_id(name),
            short_name: legacy_symbol_id(short_name),
            kind,
            definition: TyConDefinition::Primitive,
        },
    )
}

fn tycon_id_for_data_definition(name: &str) -> TyConId {
    if name == COMPILER_BOOL_NAME {
        PRIM_TYS.bool
    } else {
        legacy_tycon_id(name)
    }
}

pub fn register_symbol_display_name(symbol: Symbol, name: &str) {
    let mut registry = legacy_term_name_registry()
        .lock()
        .expect("ICE: legacy term-name registry lock poisoned");
    registry.symbol_names.insert(symbol, name.to_string());
}

pub fn symbol_display_name(symbol: Symbol) -> String {
    let registry = legacy_term_name_registry()
        .lock()
        .expect("ICE: legacy term-name registry lock poisoned");

    if let Some(name) = registry.symbol_names.get(&symbol) {
        return name.clone();
    }

    if symbol.0 & LEGACY_SYMBOL_TAG == 0 {
        return format!("s{}", symbol.0);
    }

    let index = (symbol.0 & !LEGACY_SYMBOL_TAG) as usize;
    registry
        .symbols
        .get(index)
        .cloned()
        .unwrap_or_else(|| format!("s{}", symbol.0))
}

pub fn register_def_display_name(def_id: DefId, name: &str) {
    let mut registry = legacy_term_name_registry()
        .lock()
        .expect("ICE: legacy term-name registry lock poisoned");
    registry.def_names.insert(def_id, name.to_string());
}

pub fn legacy_def_id(name: &str) -> DefId {
    let def_id = DefId {
        module: LEGACY_DEF_MODULE,
        name: legacy_symbol_id(name),
    };
    register_def_display_name(def_id, name);
    def_id
}

pub fn def_display_name(def_id: DefId) -> String {
    let registry = legacy_term_name_registry()
        .lock()
        .expect("ICE: legacy term-name registry lock poisoned");

    if let Some(name) = registry.def_names.get(&def_id) {
        return name.clone();
    }

    drop(registry);

    if def_id.module == LEGACY_DEF_MODULE {
        symbol_display_name(def_id.name)
    } else {
        format!("D{}.{}", def_id.module.0, def_id.name.0)
    }
}

pub fn var_display_name(var: &VarRef) -> String {
    match var {
        VarRef::Local(symbol) => symbol_display_name(*symbol),
        VarRef::Global(def_id) => def_display_name(*def_id),
    }
}

impl VarRef {
    pub fn legacy_local(name: &str) -> Self {
        Self::Local(legacy_symbol_id(name))
    }

    pub fn legacy_global(name: &str) -> Self {
        Self::Global(legacy_def_id(name))
    }

    pub fn display_name(&self) -> String {
        var_display_name(self)
    }
}

const LEGACY_SOURCE_TYVAR_TAG: u32 = 0x8000_0000;
const LEGACY_SOURCE_TYVAR_LEN_SHIFT: u32 = 24;
const LEGACY_SOURCE_TYVAR_CODE_MASK: u32 = 0x00ff_ffff;
const LEGACY_SOURCE_TYVAR_BASE: u32 = 40;
const LEGACY_SOURCE_TYVAR_MAX_CHARS: usize = 4;

pub fn legacy_tyvar_name(base: &str, id: TyVarId) -> String {
    format!("{}{}", base, id.0)
}

fn encode_tyvar_char(ch: char) -> Option<u32> {
    match ch {
        'a'..='z' => Some(ch as u32 - 'a' as u32 + 1),
        '0'..='9' => Some(ch as u32 - '0' as u32 + 27),
        '_' => Some(37),
        '\'' => Some(38),
        '?' => Some(39),
        _ => None,
    }
}

fn decode_tyvar_char(code: u32) -> Option<char> {
    match code {
        1..=26 => char::from_u32('a' as u32 + code - 1),
        27..=36 => char::from_u32('0' as u32 + code - 27),
        37 => Some('_'),
        38 => Some('\''),
        39 => Some('?'),
        _ => None,
    }
}

pub fn legacy_tyvar_id(name: &str) -> TyVarId {
    let chars = name.chars().collect::<Vec<_>>();
    if !chars.is_empty() && chars.len() <= LEGACY_SOURCE_TYVAR_MAX_CHARS {
        let mut code = 0_u32;
        let mut encodable = true;

        for ch in &chars {
            if let Some(part) = encode_tyvar_char(*ch) {
                code = code * LEGACY_SOURCE_TYVAR_BASE + part;
            } else {
                encodable = false;
                break;
            }
        }

        if encodable {
            return TyVarId(
                LEGACY_SOURCE_TYVAR_TAG
                    | ((chars.len() as u32) << LEGACY_SOURCE_TYVAR_LEN_SHIFT)
                    | code,
            );
        }
    }

    let hash = name.bytes().fold(2166136261_u32, |acc, byte| {
        (acc ^ byte as u32).wrapping_mul(16777619)
    });
    TyVarId(
        LEGACY_SOURCE_TYVAR_TAG
            | (5 << LEGACY_SOURCE_TYVAR_LEN_SHIFT)
            | (hash & LEGACY_SOURCE_TYVAR_CODE_MASK),
    )
}

pub fn tyvar_display_name(id: TyVarId) -> String {
    if id.0 & LEGACY_SOURCE_TYVAR_TAG == 0 {
        return legacy_tyvar_name("t", id);
    }

    let len = ((id.0 >> LEGACY_SOURCE_TYVAR_LEN_SHIFT) & 0x7) as usize;
    if !(1..=LEGACY_SOURCE_TYVAR_MAX_CHARS).contains(&len) {
        return legacy_tyvar_name("t", id);
    }

    let mut code = id.0 & LEGACY_SOURCE_TYVAR_CODE_MASK;
    let mut chars = Vec::with_capacity(len);
    for _ in 0..len {
        let part = code % LEGACY_SOURCE_TYVAR_BASE;
        let Some(ch) = decode_tyvar_char(part) else {
            return legacy_tyvar_name("t", id);
        };
        chars.push(ch);
        code /= LEGACY_SOURCE_TYVAR_BASE;
    }

    chars.iter().rev().collect()
}

impl Ty {
    pub fn is_mono(&self) -> bool {
        match self {
            TypeExpr::TyForAll(_, _, _) => false,
            TypeExpr::TyArr(lhs, rhs) | TypeExpr::TyApp(lhs, rhs) => lhs.is_mono() && rhs.is_mono(),
            _ => true,
        }
    }

    pub fn occurs_check(&self, evar_id: EvarId) -> bool {
        match self {
            Self::TyExVar(id) if *id == evar_id => true,

            Self::TyArr(lhs, rhs) | Self::TyApp(lhs, rhs) => {
                lhs.occurs_check(evar_id) || rhs.occurs_check(evar_id)
            }

            Self::TyForAll(_, preds, body) => {
                preds
                    .iter()
                    .any(|pred| pred.args.iter().any(|arg| arg.occurs_check(evar_id)))
                    || body.occurs_check(evar_id)
            }

            _ => false,
        }
    }

    pub fn peel_scheme_like(&self) -> (Vec<(TyVarId, Kind)>, Vec<Predicate>, TypeExpr) {
        let mut current_ty = self.clone();
        let mut quants_with_kinds = vec![];
        let mut predicates = vec![];
        while let TypeExpr::TyForAll(mut quants, mut preds, body) = current_ty {
            quants_with_kinds.append(&mut quants);
            predicates.append(&mut preds);
            current_ty = *body;
        }

        (quants_with_kinds, predicates, current_ty)
    }

    pub fn peel_arrow(&self) -> Option<(TypeExpr, TypeExpr)> {
        match self {
            Self::TyArr(fun_ty, arg_ty) => Some((*fun_ty.clone(), *arg_ty.clone())),
            _ => None,
        }
    }

    pub fn make_arr(lhs: TypeExpr, rhs: TypeExpr) -> TypeExpr {
        TypeExpr::TyArr(Box::new(lhs), Box::new(rhs))
    }

    pub fn make_app(lhs: TypeExpr, rhs: TypeExpr) -> TypeExpr {
        TypeExpr::TyApp(Box::new(lhs), Box::new(rhs))
    }

    pub fn make_var(var_name: String) -> TypeExpr {
        Self::TyVar(legacy_tyvar_id(&var_name))
    }

    pub fn make_var_id(base: &str, id: TyVarId) -> TypeExpr {
        let _ = base;
        Self::TyVar(id)
    }

    pub fn make_evar(id: EvarId) -> TypeExpr {
        Self::TyExVar(id)
    }

    pub fn make_con(name: String) -> TypeExpr {
        Self::TyCon(legacy_tycon_id(&name))
    }

    pub fn make_con_id(id: TyConId) -> TypeExpr {
        Self::TyCon(id)
    }

    pub fn make_forall(quant_name: String, kind: Kind, body: TypeExpr) -> TypeExpr {
        Self::make_forall_id(legacy_tyvar_id(&quant_name), kind, body)
    }

    pub fn make_forall_id(quant_id: TyVarId, kind: Kind, body: TypeExpr) -> TypeExpr {
        Self::make_forall_group(vec![(quant_id, kind)], Vec::new(), body)
    }

    pub fn make_qualified(predicates: Vec<Predicate>, body: TypeExpr) -> TypeExpr {
        Self::make_forall_group(Vec::new(), predicates, body)
    }

    pub fn make_forall_group(
        mut binders: Vec<(TyVarId, Kind)>,
        mut predicates: Vec<Predicate>,
        body: TypeExpr,
    ) -> TypeExpr {
        if binders.is_empty() && predicates.is_empty() {
            return body;
        }

        match body {
            TypeExpr::TyForAll(mut body_binders, mut body_predicates, body_body) => {
                binders.append(&mut body_binders);
                predicates.append(&mut body_predicates);
                TypeExpr::TyForAll(binders, predicates, body_body)
            }
            body => TypeExpr::TyForAll(binders, predicates, Box::new(body)),
        }
    }

    pub fn substitute(&self, var_id: TyVarId, with_type: &TypeExpr) -> TypeExpr {
        match self {
            Self::TyVar(sub_var_id) => {
                if *sub_var_id == var_id {
                    with_type.clone()
                } else {
                    self.clone()
                }
            }

            Self::TyCon(_) | Self::TyExVar(_) | Self::TyUnit => self.clone(),

            Self::TyArr(lhs, rhs) => Self::make_arr(
                lhs.substitute(var_id, with_type),
                rhs.substitute(var_id, with_type),
            ),

            Self::TyApp(lhs, rhs) => Self::make_app(
                lhs.substitute(var_id, with_type),
                rhs.substitute(var_id, with_type),
            ),

            Self::TyForAll(binders, preds, body) => {
                if binders.iter().any(|(quantifier, _)| *quantifier == var_id) {
                    Self::make_forall_group(binders.clone(), preds.clone(), (**body).clone())
                } else {
                    let new_preds = preds
                        .iter()
                        .map(|pred| Predicate {
                            class_id: pred.class_id,
                            args: pred
                                .args
                                .iter()
                                .map(|arg| arg.substitute(var_id, with_type))
                                .collect(),
                        })
                        .collect();
                    Self::make_forall_group(
                        binders.clone(),
                        new_preds,
                        body.substitute(var_id, with_type),
                    )
                }
            }
        }
    }

    pub fn substitute_evar(&self, evar_id: EvarId, with_type: &TypeExpr) -> TypeExpr {
        match self {
            Self::TyExVar(id) => {
                if *id == evar_id {
                    with_type.clone()
                } else {
                    self.clone()
                }
            }

            Self::TyVar(_) | Self::TyCon(_) | Self::TyUnit => self.clone(),

            Self::TyArr(lhs, rhs) => Self::make_arr(
                lhs.substitute_evar(evar_id, with_type),
                rhs.substitute_evar(evar_id, with_type),
            ),

            Self::TyApp(lhs, rhs) => Self::make_app(
                lhs.substitute_evar(evar_id, with_type),
                rhs.substitute_evar(evar_id, with_type),
            ),

            Self::TyForAll(binders, preds, body) => {
                let new_preds = preds
                    .iter()
                    .map(|pred| Predicate {
                        class_id: pred.class_id,
                        args: pred
                            .args
                            .iter()
                            .map(|arg| arg.substitute_evar(evar_id, with_type))
                            .collect(),
                    })
                    .collect();

                Self::make_forall_group(
                    binders.clone(),
                    new_preds,
                    body.substitute_evar(evar_id, with_type),
                )
            }
        }
    }
}

impl Pretty<Ty> for Ty {
    fn get_precedence(t: &Ty) -> u8 {
        match t {
            TypeExpr::TyForAll(_, _, _) => PRECEDENCE_LOOSE,
            TypeExpr::TyArr(_, _) => PRECEDENCE_ARR,
            TypeExpr::TyApp(_, _) => PRECEDENCE_APP,
            TypeExpr::TyUnit | TypeExpr::TyVar(_) | TypeExpr::TyExVar(_) | TypeExpr::TyCon(_) => {
                PRECEDENCE_ATOM
            }
        }
    }

    fn pretty(&self) -> String {
        fn traverse_helper(t: &Ty, parent_precedence: u8) -> String {
            let curr_precedence = TypeExpr::get_precedence(t);

            let type_fmt = match t {
                TypeExpr::TyUnit => "()".into(),
                TypeExpr::TyVar(id) => tyvar_display_name(*id),
                TypeExpr::TyExVar(id) => legacy_evar_name(*id),
                TypeExpr::TyCon(id) => tycon_fallback_name(*id),

                TypeExpr::TyArr(lhs, rhs) => {
                    let lhs_fmt = traverse_helper(lhs, PRECEDENCE_APP);
                    let rhs_fmt = traverse_helper(rhs, PRECEDENCE_ARR);
                    format!("{lhs_fmt} -> {rhs_fmt}")
                }

                TypeExpr::TyForAll(_, _, _) => {
                    let (kinded_quants, preds, rest) = t.peel_scheme_like();

                    let peeled_quants = kinded_quants
                        .iter()
                        .map(|(id, kind)| {
                            format!("({} : {})", tyvar_display_name(*id), kind.pretty())
                        })
                        .collect::<Vec<_>>();

                    let prefix = if peeled_quants.is_empty() {
                        String::new()
                    } else {
                        format!("∀ {}. ", peeled_quants.join(" "))
                    };

                    if preds.is_empty() {
                        format!("{prefix}{}", traverse_helper(&rest, PRECEDENCE_LOOSE))
                    } else {
                        let preds_fmt = if preds.len() == 1 {
                            preds[0].pretty()
                        } else {
                            format!(
                                "({})",
                                preds
                                    .iter()
                                    .map(|p| p.pretty())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        };

                        format!(
                            "{prefix}{preds_fmt} => {}",
                            traverse_helper(&rest, PRECEDENCE_LOOSE)
                        )
                    }
                }

                TypeExpr::TyApp(f, x) => {
                    let f_s = traverse_helper(f, PRECEDENCE_APP);
                    let x_s = traverse_helper(x, PRECEDENCE_ATOM);
                    format!("{f_s} {x_s}")
                }
            };

            if curr_precedence < parent_precedence {
                format!("({type_fmt})")
            } else {
                type_fmt
            }
        }

        traverse_helper(self, PRECEDENCE_LOOSE)
    }
}

impl Predicate {
    pub fn pretty(&self) -> String {
        let args = self
            .args
            .iter()
            .map(|arg| {
                let s = arg.pretty();
                match arg {
                    TypeExpr::TyArr(_, _) | TypeExpr::TyForAll(_, _, _) => format!("({s})"),
                    _ => s,
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        if args.is_empty() {
            class_fallback_name(self.class_id)
        } else {
            format!("{} {args}", class_fallback_name(self.class_id))
        }
    }

    pub fn legacy(class_name: String, args: Vec<TypeExpr>) -> Self {
        Self {
            class_id: legacy_class_id(&class_name),
            args,
        }
    }

    pub fn class_name(&self) -> String {
        class_fallback_name(self.class_id)
    }
}

impl Scheme {
    pub fn to_type_expr(&self) -> TypeExpr {
        TypeExpr::make_forall_group(
            self.quantifiers.clone(),
            self.predicates.clone(),
            self.ty.clone(),
        )
    }

    pub fn pretty(&self) -> String {
        self.to_type_expr().pretty()
    }

    pub fn mono(ty: TypeExpr) -> Self {
        Self {
            quantifiers: Vec::new(),
            predicates: Vec::new(),
            ty,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExprNode<T, A> {
    pub node: T,
    pub anno: A,
}

impl<T, A> ExprNode<T, A> {
    pub fn new(node: T, anno: A) -> Self {
        Self { node, anno }
    }
}

pub type TermExprNode<A, P = A> = ExprNode<TermExpr<A, P>, A>;
pub type PatternNode<A> = ExprNode<Pattern<A>, A>;
pub type CaseArmNode<A, P = A> = ExprNode<CaseArm<A, P>, P>;

impl<A, P> ExprNode<TermExpr<A, P>, A> {
    pub fn map_anno<B, Q, FE, FP>(
        self,
        expr_anno: &mut FE,
        pattern_anno: &mut FP,
    ) -> ExprNode<TermExpr<B, Q>, B>
    where
        FE: FnMut(A) -> B,
        FP: FnMut(P) -> Q,
    {
        let anno = expr_anno(self.anno);
        let node = match self.node {
            TermExpr::Unit => TermExpr::Unit,
            TermExpr::Var(var) => TermExpr::Var(var),
            TermExpr::Lit(lit) => TermExpr::Lit(lit),
            TermExpr::Hole => TermExpr::Hole,
            TermExpr::App(func, arg) => TermExpr::App(
                Box::new(func.map_anno(expr_anno, pattern_anno)),
                Box::new(arg.map_anno(expr_anno, pattern_anno)),
            ),
            TermExpr::Abs(param, annotation, body) => TermExpr::Abs(
                param,
                annotation,
                Box::new(body.map_anno(expr_anno, pattern_anno)),
            ),
            TermExpr::TyAbs(param, kind, body) => TermExpr::TyAbs(
                param,
                kind,
                Box::new(body.map_anno(expr_anno, pattern_anno)),
            ),
            TermExpr::TyApp(inner, ty) => {
                TermExpr::TyApp(Box::new(inner.map_anno(expr_anno, pattern_anno)), ty)
            }
            TermExpr::EvAbs(ev, pred, body) => {
                TermExpr::EvAbs(ev, pred, Box::new(body.map_anno(expr_anno, pattern_anno)))
            }
            TermExpr::EvApp(inner, ev) => {
                TermExpr::EvApp(Box::new(inner.map_anno(expr_anno, pattern_anno)), ev)
            }
            TermExpr::Anno(inner, ty) => {
                TermExpr::Anno(Box::new(inner.map_anno(expr_anno, pattern_anno)), ty)
            }
            TermExpr::Con(id) => TermExpr::Con(id),
            TermExpr::Case(scrutinee, arms) => TermExpr::Case(
                Box::new(scrutinee.map_anno(expr_anno, pattern_anno)),
                arms.into_iter()
                    .map(|arm| arm.map_anno(expr_anno, pattern_anno))
                    .collect(),
            ),
            TermExpr::IfThenElse(cond, then_branch, else_branch) => TermExpr::IfThenElse(
                Box::new(cond.map_anno(expr_anno, pattern_anno)),
                Box::new(then_branch.map_anno(expr_anno, pattern_anno)),
                Box::new(else_branch.map_anno(expr_anno, pattern_anno)),
            ),
            TermExpr::Let(name, binder, in_expr) => TermExpr::Let(
                name,
                Box::new(binder.map_anno(expr_anno, pattern_anno)),
                Box::new(in_expr.map_anno(expr_anno, pattern_anno)),
            ),
        };

        ExprNode::new(node, anno)
    }
}

impl<A> ExprNode<Pattern<A>, A> {
    pub fn map_anno<B, F>(self, anno: &mut F) -> ExprNode<Pattern<B>, B>
    where
        F: FnMut(A) -> B,
    {
        let mapped_anno = anno(self.anno);
        let node = match self.node {
            Pattern::Con(id, args) => {
                Pattern::Con(id, args.into_iter().map(|arg| arg.map_anno(anno)).collect())
            }
            Pattern::Lit(lit) => Pattern::Lit(lit),
            Pattern::Var(name) => Pattern::Var(name),
            Pattern::Wild => Pattern::Wild,
        };

        ExprNode::new(node, mapped_anno)
    }
}

impl<A, P> ExprNode<CaseArm<A, P>, P> {
    pub fn map_anno<B, Q, FE, FP>(
        self,
        expr_anno: &mut FE,
        pattern_anno: &mut FP,
    ) -> ExprNode<CaseArm<B, Q>, Q>
    where
        FE: FnMut(A) -> B,
        FP: FnMut(P) -> Q,
    {
        let anno = pattern_anno(self.anno);
        let node = CaseArm {
            pattern: self.node.pattern.map_anno(pattern_anno),
            expr: self.node.expr.map_anno(expr_anno, pattern_anno),
        };

        ExprNode::new(node, anno)
    }
}

#[derive(Clone, Debug)]
pub enum Literal {
    Int(i64),
    Float(f64),
    Char(char),
    Str(StrLitId),
}

#[derive(Clone, Debug, Copy)]
pub enum BinOp {
    Plus,
    Minus,
    Mult,
    Div,
    Mod,

    Less,
    Greater,
    LessOrEqual,
    GreaterOrEqual,
    Equal,
}

#[derive(Clone, Debug, Copy)]
pub enum UnOp {
    Neg,
}

#[derive(Clone, Debug)]
pub enum TermExpr<A, P = A> {
    Unit,
    Var(VarRef),
    Lit(Literal),
    Hole,
    App(Box<TermExprNode<A, P>>, Box<TermExprNode<A, P>>),
    Abs(String, Option<TypeExpr>, Box<TermExprNode<A, P>>),
    TyAbs(String, Kind, Box<TermExprNode<A, P>>),
    TyApp(Box<TermExprNode<A, P>>, TypeExpr),
    EvAbs(EvVarId, Predicate, Box<TermExprNode<A, P>>),
    EvApp(Box<TermExprNode<A, P>>, Ev),
    Anno(Box<TermExprNode<A, P>>, TypeExpr),
    Con(CtorId),
    Case(Box<TermExprNode<A, P>>, Vec<CaseArmNode<A, P>>),
    IfThenElse(
        Box<TermExprNode<A, P>>,
        Box<TermExprNode<A, P>>,
        Box<TermExprNode<A, P>>,
    ),
    Let(String, Box<TermExprNode<A, P>>, Box<TermExprNode<A, P>>),
}

#[derive(Clone, Debug)]
pub enum Pattern<A> {
    Con(CtorId, Vec<PatternNode<A>>),
    Lit(Literal),
    Var(String),
    Wild,
}

#[derive(Clone, Debug)]
pub struct CaseArm<A, P = A> {
    pub pattern: PatternNode<P>,
    pub expr: TermExprNode<A, P>,
}

#[derive(Clone, Debug)]
pub struct TermDefinition<A, P = A> {
    pub id: DefId,
    pub name: String,
    pub short_name: String,
    pub signature: Option<Scheme>,
    pub body: TermExprNode<A, P>,
}

#[derive(Clone, Debug)]
pub struct BuiltinDefinition {
    pub id: DefId,
    pub name: String,
    pub short_name: String,
    pub builtin: BuiltinId,
    pub arity: usize,
    pub scheme: Scheme,
}

impl<A, P> TermDefinition<A, P> {
    pub fn map_anno<B, Q, FE, FP>(
        self,
        expr_anno: &mut FE,
        pattern_anno: &mut FP,
    ) -> TermDefinition<B, Q>
    where
        FE: FnMut(A) -> B,
        FP: FnMut(P) -> Q,
    {
        TermDefinition {
            id: self.id,
            name: self.name,
            short_name: self.short_name,
            signature: self.signature,
            body: self.body.map_anno(expr_anno, pattern_anno),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedModule<A, P = A> {
    pub data_defs: Vec<DataDefinition>,
    pub class_defs: Vec<ClassDefinition>,
    pub instance_defs: Vec<InstanceDefinition<A, P>>,
    pub builtin_defs: Vec<BuiltinDefinition>,
    pub tycon_table: TyConTable,
    pub ctor_table: CtorTable,
    pub class_table: ClassTable,
    pub tycon_kinds: HashMap<TyConId, Kind>,
    pub constructor_types: HashMap<CtorId, TypeExpr>,
    pub func_schemes: HashMap<DefId, Scheme>,
    pub func_defs: Vec<TermDefinition<A, P>>,
}

impl<A, P> ResolvedModule<A, P> {
    pub fn rebuild_identity_tables(&mut self) {
        let (tycon_table, ctor_table, class_table) =
            identity_tables_from_defs(&self.data_defs, &self.class_defs);

        self.tycon_table = tycon_table;
        self.ctor_table = ctor_table;
        self.class_table = class_table;
    }

    pub fn map_anno<B, Q, FE, FP>(
        self,
        expr_anno: &mut FE,
        pattern_anno: &mut FP,
    ) -> ResolvedModule<B, Q>
    where
        FE: FnMut(A) -> B,
        FP: FnMut(P) -> Q,
    {
        ResolvedModule {
            data_defs: self.data_defs,
            class_defs: self.class_defs,
            instance_defs: self
                .instance_defs
                .into_iter()
                .map(|inst| inst.map_anno(expr_anno, pattern_anno))
                .collect(),
            builtin_defs: self.builtin_defs,
            tycon_table: self.tycon_table,
            ctor_table: self.ctor_table,
            class_table: self.class_table,
            tycon_kinds: self.tycon_kinds,
            constructor_types: self.constructor_types,
            func_schemes: self.func_schemes,
            func_defs: self
                .func_defs
                .into_iter()
                .map(|def| def.map_anno(expr_anno, pattern_anno))
                .collect(),
        }
    }
}

pub fn identity_tables_from_defs(
    data_defs: &[DataDefinition],
    class_defs: &[ClassDefinition],
) -> (TyConTable, CtorTable, ClassTable) {
    let mut tycon_table = TyConTable::new();
    let mut ctor_table = CtorTable::new();
    let mut class_table = ClassTable::new();
    let mut next_ctor_tag = 0_u16;

    for (id, info) in compiler_primitive_tycon_infos() {
        tycon_table.insert_with_id(id, info);
    }

    if let Some(bool_def) = data_defs
        .iter()
        .find(|data_def| data_def.name == COMPILER_BOOL_NAME)
    {
        insert_data_definition_identity(
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
                name: legacy_symbol_id(&class_def.name),
                short_name: legacy_symbol_id(&class_def.short_name),
                definition: class_def.clone(),
            },
        );
    }

    (tycon_table, ctor_table, class_table)
}

fn insert_data_definition_identity(
    data_def: &DataDefinition,
    tycon_table: &mut TyConTable,
    ctor_table: &mut CtorTable,
    next_ctor_tag: &mut u16,
) {
    let tycon_id = tycon_id_for_data_definition(&data_def.name);

    tycon_table.insert_with_id(
        tycon_id,
        TyConInfo {
            name: legacy_symbol_id(&data_def.name),
            short_name: legacy_symbol_id(&data_def.short_name),
            kind: data_def.tycon_kind.clone(),
            definition: TyConDefinition::Data(data_def.clone()),
        },
    );

    for ctor_def in &data_def.constructors {
        ctor_table.insert_with_id(
            ctor_def.id,
            CtorInfo {
                name: legacy_symbol_id(&ctor_def.name),
                short_name: legacy_symbol_id(&ctor_def.short_name),
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
    pub ty_params: Vec<(String, Kind)>,
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
        let id = TyConId(self.entries.len() as u32);
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
    pub fields: Vec<TypeExpr>,
    pub full_type: TypeExpr,
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
    PreludeListNil,
    PreludeListCons,
    PreludePair,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CtorInfo {
    pub name: Symbol,
    pub short_name: Symbol,
    pub tycon: TyConId,
    pub fields: Vec<TypeExpr>,
    pub full_type: TypeExpr,
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
        let id = CtorId(self.entries.len() as u32);
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Predicate {
    pub class_id: ClassId,
    pub args: Vec<TypeExpr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scheme {
    pub quantifiers: Vec<(TyVarId, Kind)>,
    pub predicates: Vec<Predicate>,
    pub ty: TypeExpr,
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
    pub params: Vec<(String, Kind)>,
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
        let id = ClassId(self.entries.len() as u32);
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

#[derive(Clone, Copy)]
pub struct PrettyTables<'a> {
    pub tycons: &'a TyConTable,
    pub classes: &'a ClassTable,
}

fn tycon_pretty_name(id: TyConId, tables: PrettyTables<'_>) -> String {
    tables
        .tycons
        .get(id)
        .map(|info| symbol_display_name(info.short_name))
        .unwrap_or_else(|| panic!("ICE: pretty tables missing TyConId {id:?}"))
}

fn class_pretty_name(id: ClassId, tables: PrettyTables<'_>) -> String {
    tables
        .classes
        .get(id)
        .map(|info| symbol_display_name(info.short_name))
        .unwrap_or_else(|| panic!("ICE: pretty tables missing ClassId {id:?}"))
}

impl TypeExpr {
    pub fn pretty_with_tables(&self, tables: PrettyTables<'_>) -> String {
        fn traverse_helper(t: &Ty, parent_precedence: u8, tables: PrettyTables<'_>) -> String {
            let curr_precedence = TypeExpr::get_precedence(t);

            let type_fmt = match t {
                TypeExpr::TyUnit => "()".into(),
                TypeExpr::TyVar(id) => tyvar_display_name(*id),
                TypeExpr::TyExVar(id) => legacy_evar_name(*id),
                TypeExpr::TyCon(id) => tycon_pretty_name(*id, tables),

                TypeExpr::TyArr(lhs, rhs) => {
                    let lhs_fmt = traverse_helper(lhs, PRECEDENCE_APP, tables);
                    let rhs_fmt = traverse_helper(rhs, PRECEDENCE_ARR, tables);
                    format!("{lhs_fmt} -> {rhs_fmt}")
                }

                TypeExpr::TyForAll(_, _, _) => {
                    let (kinded_quants, preds, rest) = t.peel_scheme_like();

                    let peeled_quants = kinded_quants
                        .iter()
                        .map(|(id, kind)| {
                            format!("({} : {})", tyvar_display_name(*id), kind.pretty())
                        })
                        .collect::<Vec<_>>();

                    let prefix = if peeled_quants.is_empty() {
                        String::new()
                    } else {
                        format!("∀ {}. ", peeled_quants.join(" "))
                    };

                    if preds.is_empty() {
                        format!(
                            "{prefix}{}",
                            traverse_helper(&rest, PRECEDENCE_LOOSE, tables)
                        )
                    } else {
                        let preds_fmt = if preds.len() == 1 {
                            preds[0].pretty_with_tables(tables)
                        } else {
                            format!(
                                "({})",
                                preds
                                    .iter()
                                    .map(|p| p.pretty_with_tables(tables))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        };

                        format!(
                            "{prefix}{preds_fmt} => {}",
                            traverse_helper(&rest, PRECEDENCE_LOOSE, tables)
                        )
                    }
                }

                TypeExpr::TyApp(f, x) => {
                    let f_s = traverse_helper(f, PRECEDENCE_APP, tables);
                    let x_s = traverse_helper(x, PRECEDENCE_ATOM, tables);
                    format!("{f_s} {x_s}")
                }
            };

            if curr_precedence < parent_precedence {
                format!("({type_fmt})")
            } else {
                type_fmt
            }
        }

        traverse_helper(self, PRECEDENCE_LOOSE, tables)
    }
}

impl Predicate {
    pub fn pretty_with_tables(&self, tables: PrettyTables<'_>) -> String {
        let class_name = class_pretty_name(self.class_id, tables);

        let args = self
            .args
            .iter()
            .map(|arg| {
                let s = arg.pretty_with_tables(tables);
                match arg {
                    TypeExpr::TyArr(_, _) | TypeExpr::TyForAll(_, _, _) => format!("({s})"),
                    _ => s,
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        if args.is_empty() {
            class_name
        } else {
            format!("{class_name} {args}")
        }
    }
}

impl Scheme {
    pub fn pretty_with_tables(&self, tables: PrettyTables<'_>) -> String {
        self.to_type_expr().pretty_with_tables(tables)
    }
}

#[derive(Clone, Debug)]
pub struct InstanceMethodDef<A, P = A> {
    pub name: String,
    pub body: TermExprNode<A, P>,
}

impl<A, P> InstanceMethodDef<A, P> {
    pub fn map_anno<B, Q, FE, FP>(
        self,
        expr_anno: &mut FE,
        pattern_anno: &mut FP,
    ) -> InstanceMethodDef<B, Q>
    where
        FE: FnMut(A) -> B,
        FP: FnMut(P) -> Q,
    {
        InstanceMethodDef {
            name: self.name,
            body: self.body.map_anno(expr_anno, pattern_anno),
        }
    }
}

#[derive(Clone, Debug)]
pub struct InstanceDefinition<A, P = A> {
    pub id: InstanceId,
    pub span: Range<usize>,
    pub def_module: ModuleId,
    pub class_module: Option<ModuleId>,
    pub type_module: Option<ModuleId>,
    pub context: Vec<Predicate>,
    pub context_evidence: Vec<EvVarId>,
    pub head: Predicate,
    pub superclass_evidence: Vec<Ev>,
    pub methods: Vec<InstanceMethodDef<A, P>>,
}

impl<A, P> InstanceDefinition<A, P> {
    pub fn map_anno<B, Q, FE, FP>(
        self,
        expr_anno: &mut FE,
        pattern_anno: &mut FP,
    ) -> InstanceDefinition<B, Q>
    where
        FE: FnMut(A) -> B,
        FP: FnMut(P) -> Q,
    {
        InstanceDefinition {
            id: self.id,
            span: self.span,
            def_module: self.def_module,
            class_module: self.class_module,
            type_module: self.type_module,
            context: self.context,
            context_evidence: self.context_evidence,
            head: self.head,
            superclass_evidence: self.superclass_evidence,
            methods: self
                .methods
                .into_iter()
                .map(|method| method.map_anno(expr_anno, pattern_anno))
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct InstanceInfo<A, P = A> {
    pub head: Predicate,
    pub context: Vec<Predicate>,
    pub class: ClassId,
    pub def_module: ModuleId,
    pub methods: Vec<InstanceMethodDef<A, P>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InstanceOverlap {
    pub first: InstanceId,
    pub second: InstanceId,
    pub class: ClassId,
    pub instance_args: Vec<TypeExpr>,
}

#[derive(Clone, Debug, Default)]
pub struct InstanceTable<A, P = A> {
    entries: Vec<(InstanceId, InstanceInfo<A, P>)>,
}

impl<A, P> InstanceTable<A, P> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn from_definitions(definitions: Vec<InstanceDefinition<A, P>>) -> Self {
        let mut table = Self::new();

        for definition in definitions {
            table.insert_definition(definition);
        }

        table
    }

    pub fn insert(&mut self, info: InstanceInfo<A, P>) -> InstanceId {
        let id = InstanceId(self.entries.len() as u32);
        self.entries.push((id, info));
        id
    }

    pub fn insert_definition(&mut self, definition: InstanceDefinition<A, P>) -> InstanceId {
        let InstanceDefinition {
            id,
            span: _,
            def_module,
            class_module: _,
            type_module: _,
            context,
            context_evidence: _,
            head,
            superclass_evidence: _,
            methods,
        } = definition;
        let class = head.class_id;

        self.entries.push((
            id,
            InstanceInfo {
                head,
                context,
                class,
                def_module,
                methods,
            },
        ));
        id
    }

    pub fn iter(&self) -> impl Iterator<Item = (InstanceId, &InstanceInfo<A, P>)> {
        self.entries.iter().map(|(id, info)| (*id, info))
    }

    pub fn get(&self, id: InstanceId) -> Option<&InstanceInfo<A, P>> {
        self.entries
            .iter()
            .find(|(existing, _)| *existing == id)
            .map(|(_, info)| info)
    }

    pub fn overlaps(&self) -> Vec<InstanceOverlap> {
        let mut overlaps = Vec::new();

        for (left_index, (left_id, left)) in self.entries.iter().enumerate() {
            for (right_id, right) in self.entries.iter().skip(left_index + 1) {
                if left.class != right.class || left.head.args.len() != right.head.args.len() {
                    continue;
                }

                if instance_heads_unify(&left.head, &right.head) {
                    overlaps.push(InstanceOverlap {
                        first: *left_id,
                        second: *right_id,
                        class: left.class,
                        instance_args: right.head.args.clone(),
                    });
                }
            }
        }

        overlaps
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum InstanceUnifySide {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum InstanceUnifyTy {
    Unit,
    Var(InstanceUnifySide, TyVarId),
    Evar(EvarId),
    Con(TyConId),
    App(Box<InstanceUnifyTy>, Box<InstanceUnifyTy>),
    Arr(Box<InstanceUnifyTy>, Box<InstanceUnifyTy>),
}

type InstanceUnifySubst = HashMap<(InstanceUnifySide, TyVarId), InstanceUnifyTy>;

fn instance_heads_unify(left: &Predicate, right: &Predicate) -> bool {
    if left.class_id != right.class_id || left.args.len() != right.args.len() {
        return false;
    }

    let mut subst = HashMap::new();

    left.args
        .iter()
        .zip(right.args.iter())
        .all(|(left, right)| {
            unify_instance_types(
                InstanceUnifyTy::from_type(InstanceUnifySide::Left, left),
                InstanceUnifyTy::from_type(InstanceUnifySide::Right, right),
                &mut subst,
            )
        })
}

impl InstanceUnifyTy {
    fn from_type(side: InstanceUnifySide, ty: &TypeExpr) -> Self {
        match ty {
            TypeExpr::TyUnit => Self::Unit,
            TypeExpr::TyVar(id) => Self::Var(side, *id),
            TypeExpr::TyExVar(id) => Self::Evar(*id),
            TypeExpr::TyCon(id) => Self::Con(*id),
            TypeExpr::TyApp(func, arg) => Self::App(
                Box::new(Self::from_type(side, func)),
                Box::new(Self::from_type(side, arg)),
            ),
            TypeExpr::TyArr(lhs, rhs) => Self::Arr(
                Box::new(Self::from_type(side, lhs)),
                Box::new(Self::from_type(side, rhs)),
            ),
            TypeExpr::TyForAll(_, _, body) => Self::from_type(side, body),
        }
    }
}

fn unify_instance_types(
    left: InstanceUnifyTy,
    right: InstanceUnifyTy,
    subst: &mut InstanceUnifySubst,
) -> bool {
    let left = apply_instance_unify_subst(&left, subst);
    let right = apply_instance_unify_subst(&right, subst);

    match (left, right) {
        (InstanceUnifyTy::Var(side, id), ty) => bind_instance_unify_var(side, id, ty, subst),
        (ty, InstanceUnifyTy::Var(side, id)) => bind_instance_unify_var(side, id, ty, subst),
        (InstanceUnifyTy::Unit, InstanceUnifyTy::Unit) => true,
        (InstanceUnifyTy::Evar(a), InstanceUnifyTy::Evar(b)) => a == b,
        (InstanceUnifyTy::Con(a), InstanceUnifyTy::Con(b)) => a == b,
        (
            InstanceUnifyTy::App(left_func, left_arg),
            InstanceUnifyTy::App(right_func, right_arg),
        ) => {
            unify_instance_types(*left_func, *right_func, subst)
                && unify_instance_types(*left_arg, *right_arg, subst)
        }
        (InstanceUnifyTy::Arr(left_arg, left_ret), InstanceUnifyTy::Arr(right_arg, right_ret)) => {
            unify_instance_types(*left_arg, *right_arg, subst)
                && unify_instance_types(*left_ret, *right_ret, subst)
        }
        _ => false,
    }
}

fn bind_instance_unify_var(
    side: InstanceUnifySide,
    id: TyVarId,
    ty: InstanceUnifyTy,
    subst: &mut InstanceUnifySubst,
) -> bool {
    let ty = apply_instance_unify_subst(&ty, subst);

    if ty == InstanceUnifyTy::Var(side, id) {
        return true;
    }

    if instance_unify_occurs(side, id, &ty, subst) {
        return false;
    }

    subst.insert((side, id), ty);
    true
}

fn apply_instance_unify_subst(ty: &InstanceUnifyTy, subst: &InstanceUnifySubst) -> InstanceUnifyTy {
    match ty {
        InstanceUnifyTy::Var(side, id) => subst
            .get(&(*side, *id))
            .map(|bound| apply_instance_unify_subst(bound, subst))
            .unwrap_or_else(|| ty.clone()),
        InstanceUnifyTy::App(func, arg) => InstanceUnifyTy::App(
            Box::new(apply_instance_unify_subst(func, subst)),
            Box::new(apply_instance_unify_subst(arg, subst)),
        ),
        InstanceUnifyTy::Arr(lhs, rhs) => InstanceUnifyTy::Arr(
            Box::new(apply_instance_unify_subst(lhs, subst)),
            Box::new(apply_instance_unify_subst(rhs, subst)),
        ),
        InstanceUnifyTy::Unit | InstanceUnifyTy::Evar(_) | InstanceUnifyTy::Con(_) => ty.clone(),
    }
}

fn instance_unify_occurs(
    side: InstanceUnifySide,
    id: TyVarId,
    ty: &InstanceUnifyTy,
    subst: &InstanceUnifySubst,
) -> bool {
    match apply_instance_unify_subst(ty, subst) {
        InstanceUnifyTy::Var(var_side, var_id) => var_side == side && var_id == id,
        InstanceUnifyTy::App(func, arg) | InstanceUnifyTy::Arr(func, arg) => {
            instance_unify_occurs(side, id, &func, subst)
                || instance_unify_occurs(side, id, &arg, subst)
        }
        InstanceUnifyTy::Unit | InstanceUnifyTy::Evar(_) | InstanceUnifyTy::Con(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solveig_base::Session;

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
            session.str(
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
            full_type: TypeExpr::make_con("Bool".to_string()),
            tag: 0,
        });
        let true_id = ctors.insert(CtorInfo {
            name: session.intern("True"),
            short_name: session.intern("True"),
            tycon: bool_id,
            fields: Vec::new(),
            full_type: TypeExpr::make_con("Bool".to_string()),
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
                params: vec![("a".to_string(), Kind::Star)],
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
    fn legacy_evar_bridge_round_trips_ids() {
        let id = EvarId(42);

        assert_eq!(legacy_evar_name(id), "a^42");
        assert_eq!(TypeExpr::make_evar(id), TypeExpr::TyExVar(id));
    }

    #[test]
    fn occurs_check_only_matches_existential_ids() {
        let needle = EvarId(2);

        assert!(TypeExpr::make_evar(needle).occurs_check(needle));
        assert!(!TypeExpr::make_evar(EvarId(3)).occurs_check(needle));
        assert!(!TypeExpr::make_var(legacy_evar_name(needle)).occurs_check(needle));
    }

    #[test]
    fn legacy_tyvar_bridge_names_generated_ids() {
        let id = TyVarId(9);

        assert_eq!(legacy_tyvar_name("t", id), "t9");
        assert_eq!(TypeExpr::make_var_id("t", id), TypeExpr::TyVar(id));
    }

    #[test]
    fn legacy_tycon_bridge_preserves_registered_names() {
        let name = "test.module.Widget";
        let id = legacy_tycon_id(name);

        assert_eq!(legacy_tycon_id(name), id);
        assert_ne!(legacy_tycon_id("test.module.OtherWidget"), id);
        assert_eq!(tycon_display_name(id), name);
        assert_eq!(TypeExpr::make_con(name.to_string()), TypeExpr::TyCon(id));
        assert_eq!(
            TypeExpr::make_con(name.to_string()).pretty(),
            format!("T{}", id.0)
        );
    }

    #[test]
    fn legacy_ctor_bridge_preserves_registered_names() {
        let name = "test.module.MkWidget";
        let id = legacy_ctor_id(name);

        assert_eq!(legacy_ctor_id(name), id);
        assert_ne!(legacy_ctor_id("test.module.OtherCtor"), id);
        assert_eq!(ctor_display_name(id), name);
    }

    #[test]
    fn legacy_class_bridge_preserves_registered_names() {
        let name = "test.module.EqWidget";
        let id = legacy_class_id(name);

        assert_eq!(legacy_class_id(name), id);
        assert_ne!(legacy_class_id("test.module.OtherClass"), id);
        assert_eq!(class_display_name(id), name);
        assert_eq!(Predicate::legacy(name.to_string(), Vec::new()).class_id, id);
        assert_eq!(
            Predicate::legacy(name.to_string(), Vec::new()).pretty(),
            format!("K{}", id.0)
        );
    }

    #[test]
    fn resolved_module_rebuilds_identity_tables_for_legacy_ids() {
        let tycon_name = "test.module.Widget";
        let ctor_name = "test.module.MkWidget";
        let class_name = "test.module.WidgetLike";
        let tycon_id = legacy_tycon_id(tycon_name);
        let ctor_id = legacy_ctor_id(ctor_name);
        let class_id = legacy_class_id(class_name);

        let data_def = DataDefinition {
            name: tycon_name.to_string(),
            short_name: "Widget".to_string(),
            ty_params: Vec::new(),
            tycon_kind: Kind::Star,
            constructors: vec![ConstructorDef {
                id: ctor_id,
                name: ctor_name.to_string(),
                short_name: "MkWidget".to_string(),
                fields: vec![TypeExpr::TyUnit],
                full_type: TypeExpr::make_arr(
                    TypeExpr::TyUnit,
                    TypeExpr::make_con(tycon_name.to_string()),
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

        module.rebuild_identity_tables();

        let tycon_info = module
            .tycon_table
            .get(tycon_id)
            .expect("ICE: rebuilt type-constructor table must contain Widget");
        assert_eq!(symbol_display_name(tycon_info.name), tycon_name);
        assert_eq!(tycon_info.kind, Kind::Star);

        let ctor_info = module
            .ctor_table
            .get(ctor_id)
            .expect("ICE: rebuilt constructor table must contain MkWidget");
        assert_eq!(symbol_display_name(ctor_info.name), ctor_name);
        assert_eq!(ctor_info.tycon, tycon_id);
        assert_eq!(ctor_info.tag, 0);

        let class_info = module
            .class_table
            .get(class_id)
            .expect("ICE: rebuilt class table must contain WidgetLike");
        assert_eq!(symbol_display_name(class_info.name), class_name);
        assert_eq!(class_info.definition.id, class_id);
    }

    #[test]
    fn table_backed_pretty_uses_identity_tables() {
        let tycon_id = TyConId(42);
        let class_id = ClassId(77);
        let mut tycons = TyConTable::new();
        let mut classes = ClassTable::new();

        tycons.insert_with_id(
            tycon_id,
            TyConInfo {
                name: legacy_symbol_id("test.module.TableTy"),
                short_name: legacy_symbol_id("TableTy"),
                kind: Kind::Star,
                definition: TyConDefinition::Primitive,
            },
        );
        classes.insert_with_id(
            class_id,
            ClassInfo {
                name: legacy_symbol_id("test.module.TableClass"),
                short_name: legacy_symbol_id("TableClass"),
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
            tycons: &tycons,
            classes: &classes,
        };
        let scheme = Scheme {
            quantifiers: Vec::new(),
            predicates: vec![Predicate {
                class_id,
                args: vec![TypeExpr::TyCon(tycon_id)],
            }],
            ty: TypeExpr::TyCon(tycon_id),
        };

        assert_eq!(TypeExpr::TyCon(tycon_id).pretty(), "T42");
        assert_eq!(
            scheme.pretty_with_tables(tables),
            "TableClass TableTy => TableTy"
        );
    }

    #[test]
    fn legacy_varref_bridge_preserves_local_and_global_names() {
        let local = VarRef::legacy_local("value");
        let global_name = "test.module.value";
        let global = VarRef::legacy_global(global_name);

        assert_eq!(var_display_name(&local), "value");
        assert_eq!(var_display_name(&global), global_name);

        let source_symbol = Symbol(12);
        register_symbol_display_name(source_symbol, "sourceValue");
        assert_eq!(
            var_display_name(&VarRef::Local(source_symbol)),
            "sourceValue"
        );

        let def_id = DefId {
            module: ModuleId(7),
            name: Symbol(11),
        };
        register_def_display_name(def_id, "other.module.value");

        assert_eq!(
            var_display_name(&VarRef::Global(def_id)),
            "other.module.value"
        );
    }
}
