//! Public and typed module API structures.
//! Resolution and later compiler stages use these maps to connect definitions across modules.

use super::*;

include!("module_api/module_table.rs");

#[derive(Debug, Clone, Default)]
pub struct ApiNames {
    pub terms: FxHashSet<Symbol>,
    pub types: FxHashSet<Symbol>,
    pub type_aliases: FxHashSet<Symbol>,
    pub constructors: FxHashSet<Symbol>,
    pub classes: FxHashSet<Symbol>,
}

#[derive(Debug, Clone)]
pub struct ModuleApi {
    pub package: PackageId,
    pub terms: FxHashMap<Symbol, DefId>,
    pub types: FxHashMap<Symbol, DefId>,
    pub type_aliases: FxHashMap<Symbol, DefId>,
    pub constructors: FxHashMap<Symbol, DefId>,
    pub classes: FxHashMap<Symbol, DefId>,
    pub fixities: FxHashMap<Symbol, Fixity>,
    pub docs: FxHashMap<DefId, DocComment>,

    pub class_methods: FxHashMap<Symbol, Vec<Symbol>>,
    pub data_constructors: FxHashMap<Symbol, Vec<Symbol>>,
    pub external: ApiNames,
    pub private: ApiNames,
    pub reexported: ApiNames,
}
impl ModuleApi {
    pub fn new(package: PackageId) -> Self {
        Self {
            package,
            terms: FxHashMap::default(),
            types: FxHashMap::default(),
            type_aliases: FxHashMap::default(),
            constructors: FxHashMap::default(),
            classes: FxHashMap::default(),
            fixities: FxHashMap::default(),
            docs: FxHashMap::default(),
            class_methods: FxHashMap::default(),
            data_constructors: FxHashMap::default(),
            external: ApiNames::default(),
            private: ApiNames::default(),
            reexported: ApiNames::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TypeAlias {
    pub id: DefId,
    pub params: Vec<TyVarId>,
    pub ty: Ty,
    pub span: FileSpan,
}

#[derive(Debug, Clone)]
pub struct TypedModuleApi {
    pub package: PackageId,
    pub term_schemes: FxHashMap<DefId, solveig_ast::ast::Scheme>,
    pub type_kinds: FxHashMap<DefId, Kind>,
    pub type_ids: FxHashMap<DefId, solveig_ast::ast::TyConId>,
    pub type_aliases: FxHashMap<DefId, TypeAlias>,
    pub constructor_types: FxHashMap<DefId, Ty>,
    pub constructor_ids: FxHashMap<DefId, CtorId>,
    pub class_defs: FxHashMap<DefId, solveig_ast::ast::ClassDefinition>,
    pub docs: FxHashMap<DefId, DocComment>,
    pub tycon_table: TyConTable,
    pub ctor_table: CtorTable,
    pub class_table: ClassTable,
    pub instance_defs: Vec<
        solveig_ast::ast::InstanceDefinition<
            solveig_check::inference::type_infer::TcMetadata,
            std::ops::Range<usize>,
        >,
    >,
}

impl TypedModuleApi {
    pub fn new(package: PackageId) -> Self {
        Self {
            package,
            term_schemes: FxHashMap::default(),
            type_kinds: FxHashMap::default(),
            type_ids: FxHashMap::default(),
            type_aliases: FxHashMap::default(),
            constructor_types: FxHashMap::default(),
            constructor_ids: FxHashMap::default(),
            class_defs: FxHashMap::default(),
            docs: FxHashMap::default(),
            tycon_table: TyConTable::new(),
            ctor_table: CtorTable::new(),
            class_table: ClassTable::new(),
            instance_defs: Vec::new(),
        }
    }

    fn include_doc(&self, out: &mut Self, def_id: DefId) {
        if let Some(doc) = self.docs.get(&def_id) {
            out.docs.insert(def_id, doc.clone());
        }
    }

    fn include_class_with_superclasses(&self, out: &mut Self, sym: &DefId) {
        if out.class_defs.contains_key(sym) {
            return;
        }

        let Some(class_def) = self.class_defs.get(sym) else {
            return;
        };

        out.class_defs.insert(*sym, class_def.clone());
        self.include_doc(out, *sym);

        // Keep superclass definitions from the same typed API if they are present.
        // This is what lets `import M(BoundedLike)` internally retain `OrdLike`
        // and `EqLike` when all three are defined in M.
        //
        // If a superclass comes from another module, that module must still be
        // imported in the usual way. Prelude classes are already imported
        // implicitly for ordinary user modules.
        for superclass in &class_def.superclasses {
            let Some((super_sym, _)) = self
                .class_defs
                .iter()
                .find(|(_, candidate)| candidate.id == superclass.class_id)
            else {
                continue;
            };

            self.include_class_with_superclasses(out, super_sym);
        }
    }

    pub fn extend_display_maps(&self, out: &mut TypedDisplayMaps<'_>) {
        for (sym, scheme) in &self.term_schemes {
            out.func_schemes.insert(*sym, scheme.clone());
        }

        for (sym, kind) in &self.type_kinds {
            let id = self
                .type_ids
                .get(sym)
                .copied()
                .expect("ICE: typed module API type kind missing TyConId");
            out.tycon_kinds.insert(id, kind.clone());
        }

        for (sym, ty) in &self.constructor_types {
            let id = self
                .constructor_ids
                .get(sym)
                .copied()
                .expect("ICE: typed module API constructor missing CtorId");
            out.constructor_types.insert(id, ty.clone());
        }

        for class_def in self.class_defs.values() {
            out.class_defs.insert(class_def.id, class_def.clone());
        }

        for (id, info) in self.tycon_table.iter() {
            out.tycon_table.insert_with_id(id, info.clone());
        }

        for (id, info) in self.ctor_table.iter() {
            out.ctor_table.insert_with_id(id, info.clone());
        }

        for (id, info) in self.class_table.iter() {
            out.class_table.insert_with_id(id, info.clone());
        }
    }

    pub fn type_name(&self, names: &Names, sym: DefId) -> Option<String> {
        let id = self.type_ids.get(&sym)?;
        let info = self.tycon_table.get(*id)?;
        Some(names.symbol_display_name(info.name))
    }
}

pub struct TypedDisplayMaps<'a> {
    pub func_schemes: &'a mut FxHashMap<DefId, solveig_ast::ast::Scheme>,
    pub tycon_kinds: &'a mut FxHashMap<solveig_ast::ast::TyConId, Kind>,
    pub constructor_types: &'a mut FxHashMap<CtorId, Ty>,
    pub class_defs: &'a mut FxHashMap<ClassId, solveig_ast::ast::ClassDefinition>,
    pub tycon_table: &'a mut TyConTable,
    pub ctor_table: &'a mut CtorTable,
    pub class_table: &'a mut ClassTable,
}

pub type SharedModuleApi = Arc<ModuleApi>;
pub type SharedTypedModuleApi = Arc<TypedModuleApi>;

#[derive(Debug, Clone)]
pub struct Imported<T> {
    pub qualifier: Symbol,
    pub qualifies: bool,
    pub accepts_module_name: bool,
    pub module: ModuleName,
    pub api: Arc<T>,
    pub unqualified: T,
}

pub type ImportedApi = Imported<ModuleApi>;
pub type ImportedTypedApi = Imported<TypedModuleApi>;

const COMPILER_PRELUDE_BUILTINS_SEGMENTS: [&str; 2] = ["prelude", "builtins"];
const COMPILER_PRELUDE_PRIM_SEGMENTS: [&str; 2] = ["prelude", "prim"];

#[derive(Debug, Clone, Default)]
pub struct BuiltinApi {
    pub terms: FxHashMap<Symbol, DefId>,
    pub types: FxHashMap<Symbol, DefId>,
    pub constructors: FxHashMap<Symbol, DefId>,
    pub classes: FxHashMap<Symbol, DefId>,
    pub type_ids: FxHashMap<DefId, TyConId>,
    pub constructor_ids: FxHashMap<DefId, CtorId>,
    pub builtin_defs: Vec<BuiltinDefinition>,
}

pub fn compiler_prelude_builtins_module(session: &Session) -> ModuleName {
    ModuleName(
        COMPILER_PRELUDE_BUILTINS_SEGMENTS
            .iter()
            .map(|segment| session.intern(segment))
            .collect(),
    )
}

pub fn compiler_prelude_prim_module(session: &Session) -> ModuleName {
    ModuleName(
        COMPILER_PRELUDE_PRIM_SEGMENTS
            .iter()
            .map(|segment| session.intern(segment))
            .collect(),
    )
}

pub fn compiler_builtin_api(session: &Session, modules: &ModuleTable) -> BuiltinApi {
    let mut api = BuiltinApi::default();
    let prelude_builtins = compiler_prelude_builtins_module(session);
    let Some(module_id) = modules.id(&prelude_builtins) else {
        return api;
    };

    for (name, id) in compiler_type_entries() {
        let symbol = session.intern(name);
        api.type_ids.insert(local_def_id(module_id, symbol), id);
    }

    for (name, id) in compiler_constructor_entries() {
        let symbol = session.intern(name);
        api.constructor_ids
            .insert(local_def_id(module_id, symbol), id);
    }

    api
}

pub fn seed_compiler_prelude_module_api(
    session: &Session,
    module_name: &ModuleName,
    module_id: ModuleId,
    builtins: &BuiltinApi,
    api: &mut ModuleApi,
) {
    if module_name == &compiler_prelude_builtins_module(session) {
        for (name, _id) in compiler_type_entries() {
            let symbol = session.intern(name);
            let def_id = local_def_id(module_id, symbol);
            api.types.insert(symbol, def_id);
            insert_compiler_doc(api, def_id, compiler_type_doc(name));
        }

        for (name, _id) in compiler_constructor_entries() {
            let symbol = session.intern(name);
            let def_id = local_def_id(module_id, symbol);
            api.constructors.insert(symbol, def_id);
            insert_compiler_doc(api, def_id, compiler_constructor_doc(name));
        }

        api.data_constructors.insert(
            session.intern("Bool"),
            vec![session.intern("True"), session.intern("False")],
        );
    }

    for builtin in &builtins.builtin_defs {
        if builtin.id.module == module_id {
            api.terms.insert(builtin.id.name, builtin.id);
            insert_compiler_doc(api, builtin.id, compiler_builtin_doc(&builtin.short_name));
        }
    }
}

fn insert_compiler_doc(api: &mut ModuleApi, def_id: DefId, doc: Option<&'static str>) {
    if let Some(doc) = doc {
        api.docs
            .entry(def_id)
            .or_insert_with(|| DocComment::new(doc));
    }
}

fn compiler_type_doc(name: &str) -> Option<&'static str> {
    Some(match name {
        "Int" => "Whole-number values used for counting, indexing, and integer arithmetic.",
        "Float" => "Floating-point numbers used for approximate numeric calculations.",
        "Char" => "A single character value.",
        "String" => "Text represented as a sequence of characters.",
        "IO" => "An effectful action that produces a value when run by the program entry point.",
        "Bool" => "A truth value, either True or False.",
        "ThreadId" => "An opaque identity for one lightweight thread.",
        "Chan" => "A typed FIFO channel shared by lightweight threads.",
        "Bytes" => "An immutable sequence of arbitrary bytes.",
        "Socket" => "An opaque identity for an explicitly managed TCP resource.",
        "StdGen" => "Explicit immutable state for deterministic pseudorandom generation.",
        _ => return None,
    })
}

fn compiler_constructor_doc(name: &str) -> Option<&'static str> {
    Some(match name {
        "True" => "The true boolean value.",
        "False" => "The false boolean value.",
        _ => return None,
    })
}

fn compiler_builtin_doc(name: &str) -> Option<&'static str> {
    Some(match name {
        "intAdd" => "Adds two integers.",
        "intSub" => "Subtracts the second integer from the first.",
        "intMul" => "Multiplies two integers.",
        "intDiv" => "Divides two integers using integer division.",
        "intMod" => "Returns the integer remainder after division.",
        "intNeg" => "Negates an integer.",
        "intEq" => "Checks whether two integers are equal.",
        "intLt" => "Checks whether the first integer is less than the second.",
        "intLe" => "Checks whether the first integer is less than or equal to the second.",
        "intGt" => "Checks whether the first integer is greater than the second.",
        "intGe" => "Checks whether the first integer is greater than or equal to the second.",
        "intCompare" => "Compares two integers and returns their ordering.",
        "floatAdd" => "Adds two floating-point numbers.",
        "floatSub" => "Subtracts the second floating-point number from the first.",
        "floatMul" => "Multiplies two floating-point numbers.",
        "floatDiv" => "Divides two floating-point numbers.",
        "floatNeg" => "Negates a floating-point number.",
        "floatEq" => "Checks whether two floating-point numbers are equal.",
        "floatLt" => "Checks whether the first floating-point number is less than the second.",
        "floatLe" => {
            "Checks whether the first floating-point number is less than or equal to the second."
        }
        "floatGt" => "Checks whether the first floating-point number is greater than the second.",
        "floatGe" => {
            "Checks whether the first floating-point number is greater than or equal to the second."
        }
        "intToFloat" => "Converts an integer to a floating-point number.",
        "floatToInt" => "Converts a floating-point number to an integer by truncating toward zero.",
        "intToString" => "Renders an integer as text.",
        "floatToString" => "Renders a floating-point number as text.",
        "charToString" => "Converts a character to a one-character string.",
        "stringLength" => "Counts the characters in a string.",
        "stringAppend" => "Concatenates two strings.",
        "stringSlice" => "Takes a substring starting at an index for a given character count.",
        "stringCharAt" => "Returns the character at an index when it exists.",
        "stringEq" => "Checks whether two strings are equal.",
        "stringCompare" => "Compares two strings lexicographically.",
        "charEq" => "Checks whether two characters are equal.",
        "charCompare" => "Compares two characters by their ordering.",
        "stringToChars" => "Breaks a string into a list of characters.",
        "charsToString" => "Builds a string from a list of characters.",
        "stringToBytes" => "Encodes a string as UTF-8 bytes.",
        "bytesToString" => "Decodes UTF-8 bytes or returns InvalidUtf8.",
        "bytesLength" => "Returns the number of bytes.",
        "bytesIndex" => "Returns the byte at an index when it exists.",
        "bytesSlice" => "Takes a byte slice from a starting index and length.",
        "bytesAppend" => "Concatenates two byte strings.",
        "connect" => "Opens a TCP connection to an IP address or localhost.",
        "resolveHost" => {
            "Resolves a host name to an IPv4 address without blocking other green threads."
        }
        "listen" => "Binds a loopback TCP listener and returns its actual port.",
        "accept" => "Accepts the next connection from a TCP listener.",
        "sendBytes" => "Sends an entire byte string on a connected TCP socket.",
        "recvBytes" => "Receives up to a requested number of bytes from a TCP socket.",
        "closeSocket" => "Explicitly closes a TCP listener or connection.",
        "pureIO" => "Creates an IO action that immediately returns a value.",
        "bindIO" => "Sequences an IO action and passes its result to the next action.",
        "error" => "Raises a defect from pure code.",
        "catchDefect" => "Handles a defect raised inside an installed IO scope.",
        "printLine" => "Writes text followed by a newline.",
        "printStr" => "Writes text without adding a newline.",
        "readLine" => "Reads one line of input.",
        "getArgs" => "Reads the command-line arguments passed to the program.",
        "getEnv" => "Reads a runtime-local environment variable when it is present.",
        "setEnv" => "Sets a runtime-local environment variable for this program.",
        "getEnvironment" => "Returns the runtime environment sorted by variable name.",
        "getProgName" => "Returns the current Solveig project's directory name.",
        "readFile" => "Reads a file as text.",
        "readFileLimited" => "Reads a file as text within a byte limit.",
        "readFileBytes" => "Reads an entire file as arbitrary bytes.",
        "readFileBytesLimited" => "Reads a file as arbitrary bytes within a limit.",
        "writeFile" => "Writes text to a file, replacing its previous contents.",
        "writeFileBytes" => "Writes arbitrary bytes, replacing the file's previous contents.",
        "appendFile" => "Appends text to the end of a file.",
        "getCurrentDirectory" => "Returns the runtime's logical working directory.",
        "setCurrentDirectory" => "Changes the runtime's logical working directory.",
        "doesFileExist" => "Checks whether a path names a regular file.",
        "doesDirectoryExist" => "Checks whether a path names a directory.",
        "listDirectory" => "Lists immediate directory entries in lexical order.",
        "createDirectory" => "Creates one directory beneath an existing parent.",
        "removeFile" => "Removes one file.",
        "renameFile" => "Renames a file or directory.",
        "getFileSize" => "Returns a file's size in bytes.",
        "getModificationTime" => "Returns modification time as nanoseconds since the Unix epoch.",
        "getMonotonicTime" => "Returns nanoseconds from a runtime-local monotonic origin.",
        "getSystemTime" => "Returns signed nanoseconds since the Unix epoch.",
        "mkStdGen" => "Creates a deterministic pseudorandom generator from an integer seed.",
        "newStdGen" => "Creates a fresh runtime-seeded pseudorandom generator.",
        "randomInt" => "Produces an integer and the next explicit generator state.",
        "randomRange" => "Produces an integer in an inclusive range and the next generator state.",
        "runProcess" => "Runs a child process and strictly captures its output and exit code.",
        "runProcessLimited" => "Runs a child process with bounded output capture.",
        "exitWith" => "Terminates the main program with a requested host status after cleanup.",
        "exitSuccess" => "Terminates the main program successfully after cleanup.",
        "exitFailure" => "Terminates the main program unsuccessfully after cleanup.",
        "processExitCode" => "Returns a captured child process's exit code.",
        "processStdout" => "Returns a captured child process's standard output.",
        "processStderr" => "Returns a captured child process's standard error.",
        "openFile" => "Opens an explicitly owned text-file handle in the selected mode.",
        "hClose" => "Flushes and explicitly closes a handle.",
        "hFlush" => "Flushes buffered handle output to its host stream.",
        "hGetLine" => "Reads one UTF-8 line without its line terminator.",
        "hGetContents" => "Strictly reads all remaining UTF-8 text from a handle.",
        "hGetBytes" => "Reads at most a requested number of arbitrary bytes from a handle.",
        "hPutStr" => "Writes UTF-8 text through a handle's buffering policy.",
        "hPutStrLn" => "Writes UTF-8 text and a newline through a handle.",
        "hPutBytes" => "Writes arbitrary bytes through a handle's buffering policy.",
        "hIsEOF" => "Checks for end of input without consuming a byte.",
        "hSetBuffering" => "Flushes and changes a handle's buffering policy.",
        "stdin" => "The process standard-input handle.",
        "stdout" => "The process standard-output handle.",
        "stderr" => "The process standard-error handle.",
        "withFile" => "Opens a file and closes it after success, failure, defect, or interruption.",
        "spawn" => "Starts an IO action in a new lightweight thread.",
        "newChan" => "Creates an unbounded typed channel.",
        "newBoundedChan" => "Creates a typed channel with a positive capacity.",
        "send" => "Sends a value through a typed channel.",
        "recv" => "Receives the oldest value from a typed channel.",
        "tryRecv" => "Receives a channel value without blocking.",
        "closeChan" => "Closes a typed channel.",
        "killThread" => "Requests interruption of a lightweight thread.",
        "sleep" => "Suspends the current lightweight thread for a duration.",
        "withThread" => "Runs a child action that cannot outlive its enclosing scope.",
        "race" => "Returns the first of two concurrent results and cancels the loser.",
        "concurrently" => "Runs two actions concurrently and returns both results.",
        "timeout" => "Returns Nothing when an action exceeds its millisecond deadline.",
        "maskIO" => "Defers interruption while an IO action is runnable.",
        "restoreMaskIO" => "Temporarily restores the enclosing interruption state.",
        _ => return None,
    })
}

fn compiler_type_entries() -> [(&'static str, TyConId); 12] {
    [
        ("Int", PRIM_TYS.int),
        ("Float", PRIM_TYS.float),
        ("Char", PRIM_TYS.char),
        ("String", PRIM_TYS.string),
        ("IO", PRIM_TYS.io),
        ("Bool", PRIM_TYS.bool),
        ("ThreadId", PRIM_TYS.thread_id),
        ("Chan", PRIM_TYS.chan),
        ("Bytes", PRIM_TYS.bytes),
        ("Socket", PRIM_TYS.socket),
        ("Handle", PRIM_TYS.handle),
        ("StdGen", PRIM_TYS.std_gen),
    ]
}

fn compiler_constructor_entries() -> [(&'static str, CtorId); 2] {
    [("True", PRIM_TYS.true_ctor), ("False", PRIM_TYS.false_ctor)]
}

pub(crate) fn local_def_id(module_id: ModuleId, name: Symbol) -> DefId {
    DefId {
        module: module_id,
        name,
    }
}

include!("module_api/visibility.rs");
