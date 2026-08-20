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
    pub thread_id: TyConId,
    pub chan: TyConId,
    pub bytes: TyConId,
    pub socket: TyConId,
    pub handle: TyConId,
    pub std_gen: TyConId,
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
    thread_id: TyConId(6),
    chan: TyConId(7),
    bytes: TyConId(8),
    socket: TyConId(9),
    handle: TyConId(10),
    std_gen: TyConId(11),
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
            Kind::make_arrow(Kind::Star, Kind::make_arrow(Kind::Star, Kind::Star)),
        ),
        compiler_primitive_tycon_info(
            names,
            PRIM_TYS.thread_id,
            COMPILER_THREAD_ID_NAME,
            "ThreadId",
            Kind::Star,
        ),
        compiler_primitive_tycon_info(
            names,
            PRIM_TYS.chan,
            COMPILER_CHAN_NAME,
            "Chan",
            Kind::make_arrow(Kind::Star, Kind::Star),
        ),
        compiler_primitive_tycon_info(
            names,
            PRIM_TYS.bytes,
            COMPILER_BYTES_NAME,
            "Bytes",
            Kind::Star,
        ),
        compiler_primitive_tycon_info(
            names,
            PRIM_TYS.socket,
            COMPILER_SOCKET_NAME,
            "Socket",
            Kind::Star,
        ),
        compiler_primitive_tycon_info(
            names,
            PRIM_TYS.handle,
            COMPILER_HANDLE_NAME,
            "Handle",
            Kind::Star,
        ),
        compiler_primitive_tycon_info(
            names,
            PRIM_TYS.std_gen,
            COMPILER_STD_GEN_NAME,
            "StdGen",
            Kind::Star,
        ),
    ]
}

pub fn compiler_primitive_tycon_kinds() -> FxHashMap<TyConId, Kind> {
    FxHashMap::from_iter([
        (PRIM_TYS.int, Kind::Star),
        (PRIM_TYS.float, Kind::Star),
        (PRIM_TYS.char, Kind::Star),
        (PRIM_TYS.string, Kind::Star),
        (
            PRIM_TYS.io,
            Kind::make_arrow(Kind::Star, Kind::make_arrow(Kind::Star, Kind::Star)),
        ),
        (PRIM_TYS.thread_id, Kind::Star),
        (PRIM_TYS.chan, Kind::make_arrow(Kind::Star, Kind::Star)),
        (PRIM_TYS.bytes, Kind::Star),
        (PRIM_TYS.socket, Kind::Star),
        (PRIM_TYS.handle, Kind::Star),
        (PRIM_TYS.std_gen, Kind::Star),
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

pub fn prelude_io_error_tycon_id(names: &mut Names) -> TyConId {
    names.tycon_id(PRELUDE_IO_ERROR_NAME)
}

pub fn prelude_result_tycon_id(names: &mut Names) -> TyConId {
    names.tycon_id(PRELUDE_RESULT_NAME)
}

pub fn prelude_defect_tycon_id(names: &mut Names) -> TyConId {
    names.tycon_id(PRELUDE_DEFECT_NAME)
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

pub fn prelude_pair_tycon_id(names: &mut Names) -> TyConId {
    names.tycon_id(PRELUDE_PAIR_NAME)
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

pub fn std_io_mode_tycon_id(names: &mut Names) -> TyConId {
    names.tycon_id(STD_IO_MODE_NAME)
}

pub fn std_buffer_mode_tycon_id(names: &mut Names) -> TyConId {
    names.tycon_id(STD_BUFFER_MODE_NAME)
}

pub fn std_process_result_tycon_id(names: &mut Names) -> TyConId {
    names.tycon_id(STD_PROCESS_RESULT_NAME)
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
        PRELUDE_IO_ERROR_STDOUT_NAME => CtorRuntimeRole::PreludeIoErrorStdout,
        PRELUDE_IO_ERROR_STDIN_NAME => CtorRuntimeRole::PreludeIoErrorStdin,
        PRELUDE_IO_ERROR_READ_FILE_NAME => CtorRuntimeRole::PreludeIoErrorReadFile,
        PRELUDE_IO_ERROR_WRITE_FILE_NAME => CtorRuntimeRole::PreludeIoErrorWriteFile,
        PRELUDE_IO_ERROR_APPEND_FILE_NAME => CtorRuntimeRole::PreludeIoErrorAppendFile,
        PRELUDE_IO_ERROR_CHANNEL_CLOSED_NAME => CtorRuntimeRole::PreludeIoErrorChannelClosed,
        PRELUDE_IO_ERROR_INVALID_UTF8_NAME => CtorRuntimeRole::PreludeIoErrorInvalidUtf8,
        PRELUDE_IO_ERROR_RESOLVE_HOST_NAME => CtorRuntimeRole::PreludeIoErrorResolveHost,
        PRELUDE_IO_ERROR_CONNECT_NAME => CtorRuntimeRole::PreludeIoErrorConnect,
        PRELUDE_IO_ERROR_LISTEN_NAME => CtorRuntimeRole::PreludeIoErrorListen,
        PRELUDE_IO_ERROR_ACCEPT_NAME => CtorRuntimeRole::PreludeIoErrorAccept,
        PRELUDE_IO_ERROR_SEND_NAME => CtorRuntimeRole::PreludeIoErrorSend,
        PRELUDE_IO_ERROR_RECV_NAME => CtorRuntimeRole::PreludeIoErrorRecv,
        PRELUDE_IO_ERROR_CLOSE_SOCKET_NAME => CtorRuntimeRole::PreludeIoErrorCloseSocket,
        PRELUDE_IO_ERROR_OPEN_HANDLE_NAME => CtorRuntimeRole::PreludeIoErrorOpenHandle,
        PRELUDE_IO_ERROR_CLOSE_HANDLE_NAME => CtorRuntimeRole::PreludeIoErrorCloseHandle,
        PRELUDE_IO_ERROR_FLUSH_HANDLE_NAME => CtorRuntimeRole::PreludeIoErrorFlushHandle,
        PRELUDE_IO_ERROR_READ_HANDLE_NAME => CtorRuntimeRole::PreludeIoErrorReadHandle,
        PRELUDE_IO_ERROR_WRITE_HANDLE_NAME => CtorRuntimeRole::PreludeIoErrorWriteHandle,
        PRELUDE_IO_ERROR_SET_BUFFERING_NAME => CtorRuntimeRole::PreludeIoErrorSetBuffering,
        PRELUDE_IO_ERROR_GET_ENV_NAME => CtorRuntimeRole::PreludeIoErrorGetEnv,
        PRELUDE_IO_ERROR_SET_ENV_NAME => CtorRuntimeRole::PreludeIoErrorSetEnv,
        PRELUDE_IO_ERROR_GET_CURRENT_DIRECTORY_NAME => {
            CtorRuntimeRole::PreludeIoErrorGetCurrentDirectory
        }
        PRELUDE_IO_ERROR_SET_CURRENT_DIRECTORY_NAME => {
            CtorRuntimeRole::PreludeIoErrorSetCurrentDirectory
        }
        PRELUDE_IO_ERROR_DOES_FILE_EXIST_NAME => CtorRuntimeRole::PreludeIoErrorDoesFileExist,
        PRELUDE_IO_ERROR_DOES_DIRECTORY_EXIST_NAME => {
            CtorRuntimeRole::PreludeIoErrorDoesDirectoryExist
        }
        PRELUDE_IO_ERROR_LIST_DIRECTORY_NAME => CtorRuntimeRole::PreludeIoErrorListDirectory,
        PRELUDE_IO_ERROR_CREATE_DIRECTORY_NAME => CtorRuntimeRole::PreludeIoErrorCreateDirectory,
        PRELUDE_IO_ERROR_REMOVE_FILE_NAME => CtorRuntimeRole::PreludeIoErrorRemoveFile,
        PRELUDE_IO_ERROR_RENAME_FILE_NAME => CtorRuntimeRole::PreludeIoErrorRenameFile,
        PRELUDE_IO_ERROR_GET_FILE_SIZE_NAME => CtorRuntimeRole::PreludeIoErrorGetFileSize,
        PRELUDE_IO_ERROR_GET_MODIFICATION_TIME_NAME => {
            CtorRuntimeRole::PreludeIoErrorGetModificationTime
        }
        PRELUDE_IO_ERROR_RUN_PROCESS_NAME => CtorRuntimeRole::PreludeIoErrorRunProcess,
        PRELUDE_IO_ERROR_HTTP_TRANSPORT_NAME => CtorRuntimeRole::PreludeIoErrorHttpTransport,
        PRELUDE_IO_ERROR_HTTP_TLS_NAME => CtorRuntimeRole::PreludeIoErrorHttpTls,
        PRELUDE_IO_ERROR_HTTP_TIMEOUT_NAME => CtorRuntimeRole::PreludeIoErrorHttpTimeout,
        PRELUDE_IO_ERROR_HTTP_REDIRECT_NAME => CtorRuntimeRole::PreludeIoErrorHttpRedirect,
        PRELUDE_IO_ERROR_HTTP_HEADERS_TOO_LARGE_NAME => {
            CtorRuntimeRole::PreludeIoErrorHttpHeadersTooLarge
        }
        PRELUDE_IO_ERROR_HTTP_BODY_TOO_LARGE_NAME => {
            CtorRuntimeRole::PreludeIoErrorHttpBodyTooLarge
        }
        STD_PROCESS_RESULT_CTOR_NAME => CtorRuntimeRole::StdProcessResult,
        STD_IO_MODE_READ_NAME => CtorRuntimeRole::StdIoModeRead,
        STD_IO_MODE_WRITE_NAME => CtorRuntimeRole::StdIoModeWrite,
        STD_IO_MODE_APPEND_NAME => CtorRuntimeRole::StdIoModeAppend,
        STD_IO_MODE_READ_WRITE_NAME => CtorRuntimeRole::StdIoModeReadWrite,
        STD_BUFFER_MODE_NONE_NAME => CtorRuntimeRole::StdBufferModeNone,
        STD_BUFFER_MODE_LINE_NAME => CtorRuntimeRole::StdBufferModeLine,
        STD_BUFFER_MODE_BLOCK_NAME => CtorRuntimeRole::StdBufferModeBlock,
        PRELUDE_RESULT_OK_NAME => CtorRuntimeRole::PreludeResultOk,
        PRELUDE_RESULT_ERR_NAME => CtorRuntimeRole::PreludeResultErr,
        PRELUDE_DEFECT_ERROR_NAME => CtorRuntimeRole::PreludeDefectError,
        PRELUDE_DEFECT_DIVISION_BY_ZERO_NAME => CtorRuntimeRole::PreludeDefectDivisionByZero,
        PRELUDE_DEFECT_STACK_EXHAUSTED_NAME => CtorRuntimeRole::PreludeDefectStackExhausted,
        PRELUDE_DEFECT_HEAP_EXHAUSTED_NAME => CtorRuntimeRole::PreludeDefectHeapExhausted,
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
    PreludeIoErrorStdout,
    PreludeIoErrorStdin,
    PreludeIoErrorReadFile,
    PreludeIoErrorWriteFile,
    PreludeIoErrorAppendFile,
    PreludeIoErrorChannelClosed,
    PreludeIoErrorInvalidUtf8,
    PreludeIoErrorResolveHost,
    PreludeIoErrorConnect,
    PreludeIoErrorListen,
    PreludeIoErrorAccept,
    PreludeIoErrorSend,
    PreludeIoErrorRecv,
    PreludeIoErrorCloseSocket,
    PreludeIoErrorOpenHandle,
    PreludeIoErrorCloseHandle,
    PreludeIoErrorFlushHandle,
    PreludeIoErrorReadHandle,
    PreludeIoErrorWriteHandle,
    PreludeIoErrorSetBuffering,
    PreludeIoErrorGetEnv,
    PreludeIoErrorSetEnv,
    PreludeIoErrorGetCurrentDirectory,
    PreludeIoErrorSetCurrentDirectory,
    PreludeIoErrorDoesFileExist,
    PreludeIoErrorDoesDirectoryExist,
    PreludeIoErrorListDirectory,
    PreludeIoErrorCreateDirectory,
    PreludeIoErrorRemoveFile,
    PreludeIoErrorRenameFile,
    PreludeIoErrorGetFileSize,
    PreludeIoErrorGetModificationTime,
    PreludeIoErrorRunProcess,
    PreludeIoErrorHttpTransport,
    PreludeIoErrorHttpTls,
    PreludeIoErrorHttpTimeout,
    PreludeIoErrorHttpRedirect,
    PreludeIoErrorHttpHeadersTooLarge,
    PreludeIoErrorHttpBodyTooLarge,
    StdProcessResult,
    StdIoModeRead,
    StdIoModeWrite,
    StdIoModeAppend,
    StdIoModeReadWrite,
    StdBufferModeNone,
    StdBufferModeLine,
    StdBufferModeBlock,
    PreludeResultOk,
    PreludeResultErr,
    PreludeDefectError,
    PreludeDefectDivisionByZero,
    PreludeDefectStackExhausted,
    PreludeDefectHeapExhausted,
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
