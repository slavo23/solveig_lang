//! Explicit name and literal display tables for compiler identities.
//! The driver owns this state and threads it through resolver, checker, and lowering.

use solveig_base::{FxHashMap, ice};

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
    ids: FxHashMap<String, TyConId>,
    ctor_names: Vec<String>,
    ctor_ids: FxHashMap<String, CtorId>,
    class_names: Vec<String>,
    class_ids: FxHashMap<String, ClassId>,
    symbols: Vec<String>,
    symbol_ids: FxHashMap<String, Symbol>,
    symbol_names: FxHashMap<Symbol, String>,
    def_names: FxHashMap<DefId, String>,
    string_values: Vec<String>,
    string_ids: FxHashMap<String, StrLitId>,
    tyvar_names: FxHashMap<TyVarId, String>,
    next_tyvar_id: u32,
}

/// Records display names allocated speculatively before merging them into `Names`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NameScratch {
    base_tyvar_id: u32,
    fresh_tyvars: Vec<(TyVarId, String)>,
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

    /// Returns the next type-variable id that would be allocated by this table.
    pub fn next_tyvar_id(&self) -> TyVarId {
        TyVarId(self.next_tyvar_id)
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
        self.string_values.get(id.0 as usize).map_or_else(
            || ice!("missing string literal payload for StrLitId {id:?}"),
            String::as_str,
        )
    }

    pub fn string_literals(&self) -> &[String] {
        &self.string_values
    }
}

impl NameScratch {
    /// Creates an empty scratch table whose ids continue after `names`.
    pub fn from_names(names: &Names) -> Self {
        Self {
            base_tyvar_id: names.next_tyvar_id().0,
            fresh_tyvars: Vec::new(),
        }
    }

    /// Allocates one scratch type-variable display name.
    pub fn fresh_tyvar(&mut self, display_name: &str) -> TyVarId {
        let offset = u32::try_from(self.fresh_tyvars.len())
            .expect("ICE: name scratch exceeded u32 capacity");
        let id = TyVarId(
            self.base_tyvar_id
                .checked_add(offset)
                .expect("ICE: type variable id table exceeded u32 capacity"),
        );
        self.fresh_tyvars.push((id, display_name.to_string()));
        id
    }

    /// Allocates one generated scratch type-variable display name.
    pub fn fresh_generated_tyvar(&mut self, base: &str) -> (TyVarId, String) {
        let id = TyVarId(
            self.base_tyvar_id
                .checked_add(
                    u32::try_from(self.fresh_tyvars.len())
                        .expect("ICE: name scratch exceeded u32 capacity"),
                )
                .expect("ICE: type variable id table exceeded u32 capacity"),
        );
        let display_name = generated_tyvar_name(base, id);
        let allocated = self.fresh_tyvar(&display_name);
        debug_assert_eq!(allocated, id);
        (allocated, display_name)
    }

    /// Looks up a type-variable display name, preferring scratch entries.
    pub fn tyvar_display_name(&self, names: &Names, id: TyVarId) -> String {
        self.fresh_tyvars
            .iter()
            .rev()
            .find_map(|(fresh_id, display_name)| (*fresh_id == id).then(|| display_name.clone()))
            .unwrap_or_else(|| names.tyvar_display_name(id))
    }

    /// Captures the current scratch length for later rollback.
    pub fn checkpoint(&self) -> usize {
        self.fresh_tyvars.len()
    }

    /// Restores the scratch table to a previous checkpoint.
    pub fn restore(&mut self, checkpoint: usize) {
        self.fresh_tyvars.truncate(checkpoint);
    }

    /// Merges all scratch names into the shared name table.
    pub fn merge_into(self, names: &mut Names) {
        for (id, display_name) in self.fresh_tyvars {
            names.register_tyvar_name(id, &display_name);
        }
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
pub const COMPILER_THREAD_ID_NAME: &str = "prelude.builtins.ThreadId";
pub const COMPILER_CHAN_NAME: &str = "prelude.builtins.Chan";
pub const COMPILER_BYTES_NAME: &str = "prelude.builtins.Bytes";
pub const COMPILER_SOCKET_NAME: &str = "prelude.builtins.Socket";
pub const COMPILER_HANDLE_NAME: &str = "prelude.builtins.Handle";
pub const COMPILER_BOOL_NAME: &str = "prelude.builtins.Bool";
pub const COMPILER_TRUE_NAME: &str = "prelude.builtins.True";
pub const COMPILER_FALSE_NAME: &str = "prelude.builtins.False";
pub const PRELUDE_MAYBE_NAME: &str = "prelude.builtins.Maybe";
pub const PRELUDE_MAYBE_JUST_NAME: &str = "prelude.builtins.Just";
pub const PRELUDE_MAYBE_NOTHING_NAME: &str = "prelude.builtins.Nothing";
pub const PRELUDE_IO_ERROR_NAME: &str = "prelude.builtins.IOError";
pub const PRELUDE_IO_ERROR_STDOUT_NAME: &str = "prelude.builtins.StdoutError";
pub const PRELUDE_IO_ERROR_STDIN_NAME: &str = "prelude.builtins.StdinError";
pub const PRELUDE_IO_ERROR_READ_FILE_NAME: &str = "prelude.builtins.ReadFileError";
pub const PRELUDE_IO_ERROR_WRITE_FILE_NAME: &str = "prelude.builtins.WriteFileError";
pub const PRELUDE_IO_ERROR_APPEND_FILE_NAME: &str = "prelude.builtins.AppendFileError";
pub const PRELUDE_IO_ERROR_CHANNEL_CLOSED_NAME: &str = "prelude.builtins.ChannelClosed";
pub const PRELUDE_IO_ERROR_INVALID_UTF8_NAME: &str = "prelude.builtins.InvalidUtf8";
pub const PRELUDE_IO_ERROR_RESOLVE_HOST_NAME: &str = "prelude.builtins.ResolveHostError";
pub const PRELUDE_IO_ERROR_CONNECT_NAME: &str = "prelude.builtins.ConnectError";
pub const PRELUDE_IO_ERROR_LISTEN_NAME: &str = "prelude.builtins.ListenError";
pub const PRELUDE_IO_ERROR_ACCEPT_NAME: &str = "prelude.builtins.AcceptError";
pub const PRELUDE_IO_ERROR_SEND_NAME: &str = "prelude.builtins.SendError";
pub const PRELUDE_IO_ERROR_RECV_NAME: &str = "prelude.builtins.RecvError";
pub const PRELUDE_IO_ERROR_CLOSE_SOCKET_NAME: &str = "prelude.builtins.CloseSocketError";
pub const PRELUDE_IO_ERROR_OPEN_HANDLE_NAME: &str = "prelude.builtins.OpenHandleError";
pub const PRELUDE_IO_ERROR_CLOSE_HANDLE_NAME: &str = "prelude.builtins.CloseHandleError";
pub const PRELUDE_IO_ERROR_FLUSH_HANDLE_NAME: &str = "prelude.builtins.FlushHandleError";
pub const PRELUDE_IO_ERROR_READ_HANDLE_NAME: &str = "prelude.builtins.ReadHandleError";
pub const PRELUDE_IO_ERROR_WRITE_HANDLE_NAME: &str = "prelude.builtins.WriteHandleError";
pub const PRELUDE_IO_ERROR_SET_BUFFERING_NAME: &str = "prelude.builtins.SetBufferingError";
pub const PRELUDE_IO_ERROR_GET_ENV_NAME: &str = "prelude.builtins.GetEnvError";
pub const PRELUDE_IO_ERROR_SET_ENV_NAME: &str = "prelude.builtins.SetEnvError";
pub const PRELUDE_IO_ERROR_GET_CURRENT_DIRECTORY_NAME: &str =
    "prelude.builtins.GetCurrentDirectoryError";
pub const PRELUDE_IO_ERROR_SET_CURRENT_DIRECTORY_NAME: &str =
    "prelude.builtins.SetCurrentDirectoryError";
pub const PRELUDE_IO_ERROR_DOES_FILE_EXIST_NAME: &str = "prelude.builtins.DoesFileExistError";
pub const PRELUDE_IO_ERROR_DOES_DIRECTORY_EXIST_NAME: &str =
    "prelude.builtins.DoesDirectoryExistError";
pub const PRELUDE_IO_ERROR_LIST_DIRECTORY_NAME: &str = "prelude.builtins.ListDirectoryError";
pub const PRELUDE_IO_ERROR_CREATE_DIRECTORY_NAME: &str = "prelude.builtins.CreateDirectoryError";
pub const PRELUDE_IO_ERROR_REMOVE_FILE_NAME: &str = "prelude.builtins.RemoveFileError";
pub const PRELUDE_IO_ERROR_RENAME_FILE_NAME: &str = "prelude.builtins.RenameFileError";
pub const PRELUDE_IO_ERROR_GET_FILE_SIZE_NAME: &str = "prelude.builtins.GetFileSizeError";
pub const PRELUDE_IO_ERROR_GET_MODIFICATION_TIME_NAME: &str =
    "prelude.builtins.GetModificationTimeError";
pub const PRELUDE_IO_ERROR_RUN_PROCESS_NAME: &str = "prelude.builtins.RunProcessError";
pub const PRELUDE_PROCESS_RESULT_NAME: &str = "prelude.builtins.ProcessResult";
pub const PRELUDE_PROCESS_RESULT_CTOR_NAME: &str = "prelude.builtins.MkProcessResult";
pub const PRELUDE_IO_MODE_NAME: &str = "prelude.builtins.IOMode";
pub const PRELUDE_IO_MODE_READ_NAME: &str = "prelude.builtins.ReadMode";
pub const PRELUDE_IO_MODE_WRITE_NAME: &str = "prelude.builtins.WriteMode";
pub const PRELUDE_IO_MODE_APPEND_NAME: &str = "prelude.builtins.AppendMode";
pub const PRELUDE_IO_MODE_READ_WRITE_NAME: &str = "prelude.builtins.ReadWriteMode";
pub const PRELUDE_BUFFER_MODE_NAME: &str = "prelude.builtins.BufferMode";
pub const PRELUDE_BUFFER_MODE_NONE_NAME: &str = "prelude.builtins.NoBuffering";
pub const PRELUDE_BUFFER_MODE_LINE_NAME: &str = "prelude.builtins.LineBuffering";
pub const PRELUDE_BUFFER_MODE_BLOCK_NAME: &str = "prelude.builtins.BlockBuffering";
pub const PRELUDE_RESULT_NAME: &str = "prelude.builtins.Result";
pub const PRELUDE_RESULT_OK_NAME: &str = "prelude.builtins.Ok";
pub const PRELUDE_RESULT_ERR_NAME: &str = "prelude.builtins.Err";
pub const PRELUDE_DEFECT_NAME: &str = "prelude.builtins.Defect";
pub const PRELUDE_DEFECT_ERROR_NAME: &str = "prelude.builtins.Error";
pub const PRELUDE_DEFECT_DIVISION_BY_ZERO_NAME: &str = "prelude.builtins.DivisionByZero";
pub const PRELUDE_DEFECT_STACK_EXHAUSTED_NAME: &str = "prelude.builtins.StackExhausted";
pub const PRELUDE_DEFECT_HEAP_EXHAUSTED_NAME: &str = "prelude.builtins.HeapExhausted";
pub const PRELUDE_LIST_NAME: &str = "prelude.list.List";
pub const PRELUDE_LIST_NIL_NAME: &str = "prelude.list.Nil";
pub const PRELUDE_LIST_CONS_NAME: &str = "prelude.list.Cons";
pub const PRELUDE_PAIR_MKPAIR_NAME: &str = "prelude.builtins.MkPair";
pub const PRELUDE_PAIR_NAME: &str = "prelude.builtins.Pair";
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
pub const BUILTIN_SPAWN: BuiltinId = BuiltinId(46);
pub const BUILTIN_FAIL_IO: BuiltinId = BuiltinId(47);
pub const BUILTIN_CATCH_IO: BuiltinId = BuiltinId(48);
pub const BUILTIN_FINALLY_IO: BuiltinId = BuiltinId(49);
pub const BUILTIN_ERROR: BuiltinId = BuiltinId(50);
pub const BUILTIN_CATCH_DEFECT_IO: BuiltinId = BuiltinId(51);
pub const BUILTIN_NEW_CHAN: BuiltinId = BuiltinId(52);
pub const BUILTIN_NEW_BOUNDED_CHAN: BuiltinId = BuiltinId(53);
pub const BUILTIN_SEND_CHAN: BuiltinId = BuiltinId(54);
pub const BUILTIN_RECV_CHAN: BuiltinId = BuiltinId(55);
pub const BUILTIN_TRY_RECV_CHAN: BuiltinId = BuiltinId(56);
pub const BUILTIN_CLOSE_CHAN: BuiltinId = BuiltinId(57);
pub const BUILTIN_KILL_THREAD: BuiltinId = BuiltinId(58);
pub const BUILTIN_MASK_IO: BuiltinId = BuiltinId(59);
pub const BUILTIN_RESTORE_MASK_IO: BuiltinId = BuiltinId(60);
pub const BUILTIN_SLEEP: BuiltinId = BuiltinId(61);
pub const BUILTIN_RAISE_DEFECT_IO: BuiltinId = BuiltinId(62);
pub const BUILTIN_STRING_TO_BYTES: BuiltinId = BuiltinId(63);
pub const BUILTIN_BYTES_TO_STRING: BuiltinId = BuiltinId(64);
pub const BUILTIN_BYTES_LENGTH: BuiltinId = BuiltinId(65);
pub const BUILTIN_BYTES_INDEX: BuiltinId = BuiltinId(66);
pub const BUILTIN_BYTES_SLICE: BuiltinId = BuiltinId(67);
pub const BUILTIN_BYTES_APPEND: BuiltinId = BuiltinId(68);
pub const BUILTIN_CONNECT: BuiltinId = BuiltinId(69);
pub const BUILTIN_LISTEN: BuiltinId = BuiltinId(70);
pub const BUILTIN_ACCEPT: BuiltinId = BuiltinId(71);
pub const BUILTIN_SEND_BYTES: BuiltinId = BuiltinId(72);
pub const BUILTIN_RECV_BYTES: BuiltinId = BuiltinId(73);
pub const BUILTIN_CLOSE_SOCKET: BuiltinId = BuiltinId(74);
pub const BUILTIN_RESOLVE_HOST: BuiltinId = BuiltinId(75);
pub const BUILTIN_OPEN_FILE: BuiltinId = BuiltinId(76);
pub const BUILTIN_H_CLOSE: BuiltinId = BuiltinId(77);
pub const BUILTIN_H_FLUSH: BuiltinId = BuiltinId(78);
pub const BUILTIN_H_GET_LINE: BuiltinId = BuiltinId(79);
pub const BUILTIN_H_GET_CONTENTS: BuiltinId = BuiltinId(80);
pub const BUILTIN_H_PUT_STR: BuiltinId = BuiltinId(81);
pub const BUILTIN_H_PUT_STR_LN: BuiltinId = BuiltinId(82);
pub const BUILTIN_H_IS_EOF: BuiltinId = BuiltinId(83);
pub const BUILTIN_H_SET_BUFFERING: BuiltinId = BuiltinId(84);
pub const BUILTIN_STDIN: BuiltinId = BuiltinId(85);
pub const BUILTIN_STDOUT: BuiltinId = BuiltinId(86);
pub const BUILTIN_STDERR: BuiltinId = BuiltinId(87);
pub const BUILTIN_READ_FILE_BYTES: BuiltinId = BuiltinId(88);
pub const BUILTIN_WRITE_FILE_BYTES: BuiltinId = BuiltinId(89);
pub const BUILTIN_H_GET_BYTES: BuiltinId = BuiltinId(90);
pub const BUILTIN_H_PUT_BYTES: BuiltinId = BuiltinId(91);
pub const BUILTIN_GET_ENV: BuiltinId = BuiltinId(92);
pub const BUILTIN_SET_ENV: BuiltinId = BuiltinId(93);
pub const BUILTIN_GET_ENVIRONMENT: BuiltinId = BuiltinId(94);
pub const BUILTIN_GET_PROG_NAME: BuiltinId = BuiltinId(95);
pub const BUILTIN_GET_CURRENT_DIRECTORY: BuiltinId = BuiltinId(96);
pub const BUILTIN_SET_CURRENT_DIRECTORY: BuiltinId = BuiltinId(97);
pub const BUILTIN_DOES_FILE_EXIST: BuiltinId = BuiltinId(98);
pub const BUILTIN_DOES_DIRECTORY_EXIST: BuiltinId = BuiltinId(99);
pub const BUILTIN_LIST_DIRECTORY: BuiltinId = BuiltinId(100);
pub const BUILTIN_CREATE_DIRECTORY: BuiltinId = BuiltinId(101);
pub const BUILTIN_REMOVE_FILE: BuiltinId = BuiltinId(102);
pub const BUILTIN_RENAME_FILE: BuiltinId = BuiltinId(103);
pub const BUILTIN_GET_FILE_SIZE: BuiltinId = BuiltinId(104);
pub const BUILTIN_GET_MODIFICATION_TIME: BuiltinId = BuiltinId(105);
pub const BUILTIN_RUN_PROCESS: BuiltinId = BuiltinId(106);
pub const BUILTIN_EXIT_WITH: BuiltinId = BuiltinId(107);
pub const BUILTIN_GET_MONOTONIC_TIME: BuiltinId = BuiltinId(108);
pub const BUILTIN_GET_SYSTEM_TIME: BuiltinId = BuiltinId(109);
