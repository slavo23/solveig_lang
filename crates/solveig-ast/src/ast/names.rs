//! Explicit name and literal display tables for compiler identities.
//! The driver owns this state and threads it through resolver, checker, and lowering.

use std::collections::HashMap;

use solveig_base::Symbol;

use super::*;

const NAMED_TYCON_START: u32 = 0x8000_0000;
const NAMED_CTOR_START: u32 = 0x8000_0000;
const NAMED_CLASS_START: u32 = 0x8000_0000;
const NAMED_SYMBOL_START: u32 = 0x8000_0000;
const NAMED_DEF_MODULE: ModuleId = ModuleId(0xffff_fffe);

#[derive(Clone, Debug, Default)]
pub struct Names {
    tycon_names: Vec<String>,
    ids: HashMap<String, TyConId>,
    ctor_names: Vec<String>,
    ctor_ids: HashMap<String, CtorId>,
    class_names: Vec<String>,
    class_ids: HashMap<String, ClassId>,
    symbols: Vec<String>,
    symbol_ids: HashMap<String, Symbol>,
    symbol_names: HashMap<Symbol, String>,
    def_names: HashMap<DefId, String>,
    string_values: Vec<String>,
    string_ids: HashMap<String, StrLitId>,
    tyvar_names: HashMap<TyVarId, String>,
    next_tyvar_id: u32,
}

pub(super) fn tycon_fallback_name(id: TyConId) -> String {
    format!("T{}", id.0)
}

pub(super) fn ctor_fallback_name(id: CtorId) -> String {
    format!("C{}", id.0)
}

pub(super) fn class_fallback_name(id: ClassId) -> String {
    format!("K{}", id.0)
}

impl Names {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tycon_id(&mut self, name: &str) -> TyConId {
        if let Some(id) = self.ids.get(name) {
            return *id;
        }

        let id = TyConId(named_id(NAMED_TYCON_START, self.tycon_names.len()));
        self.tycon_names.push(name.to_string());
        self.ids.insert(name.to_string(), id);
        id
    }

    pub fn ctor_id(&mut self, name: &str) -> CtorId {
        if let Some(id) = self.ctor_ids.get(name) {
            return *id;
        }

        let id = CtorId(named_id(NAMED_CTOR_START, self.ctor_names.len()));
        self.ctor_names.push(name.to_string());
        self.ctor_ids.insert(name.to_string(), id);
        id
    }

    pub fn class_id(&mut self, name: &str) -> ClassId {
        if let Some(id) = self.class_ids.get(name) {
            return *id;
        }

        let id = ClassId(named_id(NAMED_CLASS_START, self.class_names.len()));
        self.class_names.push(name.to_string());
        self.class_ids.insert(name.to_string(), id);
        id
    }

    pub fn symbol(&mut self, name: &str) -> Symbol {
        if let Some(id) = self.symbol_ids.get(name) {
            return *id;
        }

        let id = Symbol(named_id(NAMED_SYMBOL_START, self.symbols.len()));
        self.symbols.push(name.to_string());
        self.symbol_ids.insert(name.to_string(), id);
        self.symbol_names.insert(id, name.to_string());
        id
    }

    pub fn register_symbol(&mut self, symbol: Symbol, name: &str) {
        self.symbol_names.insert(symbol, name.to_string());
    }

    pub fn register_def(&mut self, def_id: DefId, name: &str) {
        self.def_names.insert(def_id, name.to_string());
    }

    pub fn def_id(&mut self, name: &str) -> DefId {
        let def_id = DefId {
            module: NAMED_DEF_MODULE,
            name: self.symbol(name),
        };
        self.register_def(def_id, name);
        def_id
    }

    pub fn fresh_tyvar(&mut self, display_name: &str) -> TyVarId {
        let id = TyVarId(self.next_tyvar_id);
        self.next_tyvar_id = self
            .next_tyvar_id
            .checked_add(1)
            .expect("ICE: type variable id table exceeded u32 capacity");
        self.register_tyvar_name(id, display_name);
        id
    }

    pub fn fresh_generated_tyvar(&mut self, base: &str) -> (TyVarId, String) {
        let id = TyVarId(self.next_tyvar_id);
        let display_name = generated_tyvar_name(base, id);
        let allocated = self.fresh_tyvar(&display_name);
        debug_assert_eq!(allocated, id);
        (allocated, display_name)
    }

    pub fn register_tyvar_name(&mut self, id: TyVarId, display_name: &str) {
        self.tyvar_names.insert(id, display_name.to_string());
        if id.0 >= self.next_tyvar_id {
            self.next_tyvar_id =
                id.0.checked_add(1)
                    .expect("ICE: type variable id table exceeded u32 capacity");
        }
    }

    pub fn tyvar_display_name(&self, id: TyVarId) -> String {
        self.tyvar_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| tyvar_fallback_name(id))
    }

    pub fn tycon_display_name(&self, id: TyConId) -> String {
        named_display_name(id.0, NAMED_TYCON_START, &self.tycon_names)
            .unwrap_or_else(|| tycon_fallback_name(id))
    }

    pub fn ctor_display_name(&self, id: CtorId) -> String {
        named_display_name(id.0, NAMED_CTOR_START, &self.ctor_names)
            .unwrap_or_else(|| ctor_fallback_name(id))
    }

    pub fn class_display_name(&self, id: ClassId) -> String {
        named_display_name(id.0, NAMED_CLASS_START, &self.class_names)
            .unwrap_or_else(|| class_fallback_name(id))
    }

    pub fn symbol_display_name(&self, symbol: Symbol) -> String {
        self.symbol_names
            .get(&symbol)
            .cloned()
            .or_else(|| named_display_name(symbol.0, NAMED_SYMBOL_START, &self.symbols))
            .unwrap_or_else(|| format!("s{}", symbol.0))
    }

    pub fn def_display_name(&self, def_id: DefId) -> String {
        if let Some(name) = self.def_names.get(&def_id) {
            return name.clone();
        }

        if def_id.module == NAMED_DEF_MODULE {
            self.symbol_display_name(def_id.name)
        } else {
            format!("D{}.{}", def_id.module.0, def_id.name.0)
        }
    }

    pub fn var_display_name(&self, var: &VarRef) -> String {
        match var {
            VarRef::Local(symbol) => self.symbol_display_name(*symbol),
            VarRef::Global(def_id) => self.def_display_name(*def_id),
        }
    }

    pub fn intern_string_literal(&mut self, value: &str) -> StrLitId {
        if let Some(id) = self.string_ids.get(value) {
            return *id;
        }

        let id = StrLitId(
            u32::try_from(self.string_values.len())
                .expect("ICE: string literal table exceeded u32 capacity"),
        );
        self.string_values.push(value.to_string());
        self.string_ids.insert(value.to_string(), id);
        id
    }

    pub fn string_literal_value(&self, id: StrLitId) -> &str {
        self.string_values
            .get(id.0 as usize)
            .map(String::as_str)
            .unwrap_or_else(|| panic!("ICE: missing string literal payload for StrLitId {id:?}"))
    }

    pub fn string_literals(&self) -> &[String] {
        &self.string_values
    }
}

fn named_id(start: u32, index: usize) -> u32 {
    let index = u32::try_from(index).expect("ICE: name table exceeded u32 capacity");
    start
        .checked_add(index)
        .expect("ICE: name table id range overflow")
}

fn named_display_name(id: u32, start: u32, names: &[String]) -> Option<String> {
    let offset = id.checked_sub(start)?;
    names.get(offset as usize).cloned()
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
pub const BUILTIN_READ_FILE: BuiltinId = BuiltinId(43);
pub const BUILTIN_WRITE_FILE: BuiltinId = BuiltinId(44);
pub const BUILTIN_APPEND_FILE: BuiltinId = BuiltinId(45);
