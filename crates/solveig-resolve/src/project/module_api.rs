//! Public and typed module API structures.
//! Resolution and later compiler stages use these maps to connect definitions across modules.

use super::*;

#[derive(Debug, Clone)]
pub struct ModuleTable {
    names: Vec<ModuleName>,
    ids: FxHashMap<ModuleName, ModuleId>,
}

impl ModuleTable {
    pub fn new<'a>(session: &Session, modules: impl IntoIterator<Item = &'a ModuleName>) -> Self {
        let mut names = modules.into_iter().cloned().collect::<Vec<_>>();
        names.sort_by_key(|module| module.as_string(session));
        names.dedup();

        let ids = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let id = ModuleId(
                    u32::try_from(index).expect("ICE: module table exceeded u32 capacity"),
                );
                (name.clone(), id)
            })
            .collect();

        Self { names, ids }
    }

    pub fn id(&self, module: &ModuleName) -> Option<ModuleId> {
        self.ids.get(module).copied()
    }

    pub fn name(&self, id: ModuleId) -> &ModuleName {
        self.names
            .get(id.0 as usize)
            .expect("ICE: ModuleId not present in ModuleTable")
    }

    pub fn def_path(&self, def: DefId) -> DefPath<'_> {
        DefPath {
            module: self.name(def.module),
            name: def.name,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DefPath<'a> {
    module: &'a ModuleName,
    name: Symbol,
}

impl DefPath<'_> {
    pub fn render(self, session: &Session) -> String {
        if self.module.0.is_empty() {
            return session.str(self.name).to_string();
        }

        let mut rendered = self.module.as_string(session);
        rendered.push('.');
        rendered.push_str(&session.str(self.name));
        rendered
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModuleApi {
    pub terms: FxHashMap<Symbol, DefId>,
    pub types: FxHashMap<Symbol, DefId>,
    pub type_aliases: FxHashMap<Symbol, DefId>,
    pub constructors: FxHashMap<Symbol, DefId>,
    pub classes: FxHashMap<Symbol, DefId>,
    pub fixities: FxHashMap<Symbol, Fixity>,
    pub docs: FxHashMap<DefId, DocComment>,

    pub class_methods: FxHashMap<Symbol, Vec<Symbol>>,
}
impl ModuleApi {
    fn include_doc(&self, out: &mut Self, def_id: DefId) {
        if let Some(doc) = self.docs.get(&def_id) {
            out.docs.insert(def_id, doc.clone());
        }
    }

    pub fn exports_name(&self, name: Symbol) -> bool {
        self.terms.contains_key(&name)
            || self.types.contains_key(&name)
            || self.type_aliases.contains_key(&name)
            || self.constructors.contains_key(&name)
            || self.classes.contains_key(&name)
    }

    pub fn filtered(&self, spec: &ImportSpec) -> Self {
        match spec {
            ImportSpec::All => self.clone(),

            ImportSpec::Only(names) => {
                let mut out = Self::default();

                for name in names {
                    if let Some(sym) = self.terms.get(name) {
                        out.terms.insert(*name, *sym);
                        self.include_doc(&mut out, *sym);
                        if let Some(fixity) = self.fixities.get(name) {
                            out.fixities.insert(*name, *fixity);
                        }
                    }

                    if let Some(sym) = self.types.get(name) {
                        out.types.insert(*name, *sym);
                        self.include_doc(&mut out, *sym);
                    }

                    if let Some(sym) = self.type_aliases.get(name) {
                        out.type_aliases.insert(*name, *sym);
                        self.include_doc(&mut out, *sym);
                    }

                    if let Some(sym) = self.constructors.get(name) {
                        out.constructors.insert(*name, *sym);
                        self.include_doc(&mut out, *sym);
                    }

                    if let Some(sym) = self.classes.get(name) {
                        out.classes.insert(*name, *sym);
                        self.include_doc(&mut out, *sym);

                        // Solveig's current simple import rule is:
                        //
                        //   import M(Eq)
                        //
                        // makes the class and its methods available. We do not have
                        // Haskell-style C / C(..) / C(method) import syntax yet, so
                        // selecting a class imports all of its method terms.
                        if let Some(method_names) = self.class_methods.get(name) {
                            out.class_methods.insert(*name, method_names.clone());

                            for method_name in method_names {
                                if let Some(method_sym) = self.terms.get(method_name) {
                                    out.terms.insert(*method_name, *method_sym);
                                    self.include_doc(&mut out, *method_sym);
                                    if let Some(fixity) = self.fixities.get(method_name) {
                                        out.fixities.insert(*method_name, *fixity);
                                    }
                                }
                            }
                        }
                    }
                }

                out
            }
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

#[derive(Debug, Clone, Default)]
pub struct TypedModuleApi {
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

    fn include_class_methods(&self, out: &mut Self, class_def: &ClassDefinition) {
        for method in &class_def.methods {
            if let Some(scheme) = self.term_schemes.get(&method.id) {
                out.term_schemes.insert(method.id, scheme.clone());
                self.include_doc(out, method.id);
            }
        }
    }

    pub fn filtered(&self, spec: &ImportSpec) -> Self {
        match spec {
            ImportSpec::All => self.clone(),

            ImportSpec::Only(names) => {
                let mut out = Self {
                    tycon_table: self.tycon_table.clone(),
                    ctor_table: self.ctor_table.clone(),
                    class_table: self.class_table.clone(),
                    ..Self::default()
                };

                for name in names {
                    for (sym, scheme) in &self.term_schemes {
                        if &sym.name == name {
                            out.term_schemes.insert(*sym, scheme.clone());
                            self.include_doc(&mut out, *sym);
                        }
                    }

                    for (sym, kind) in &self.type_kinds {
                        if &sym.name == name {
                            out.type_kinds.insert(*sym, kind.clone());
                            self.include_doc(&mut out, *sym);
                            if let Some(id) = self.type_ids.get(sym) {
                                out.type_ids.insert(*sym, *id);
                            }
                        }
                    }

                    for (sym, alias) in &self.type_aliases {
                        if &sym.name == name {
                            out.type_aliases.insert(*sym, alias.clone());
                            self.include_doc(&mut out, *sym);
                        }
                    }

                    for (sym, ty) in &self.constructor_types {
                        if &sym.name == name {
                            out.constructor_types.insert(*sym, ty.clone());
                            self.include_doc(&mut out, *sym);
                            if let Some(id) = self.constructor_ids.get(sym) {
                                out.constructor_ids.insert(*sym, *id);
                            }
                        }
                    }
                }

                let mut selected_classes = Vec::<(DefId, bool)>::new();

                for (sym, class_def) in &self.class_defs {
                    let class_selected = names.iter().any(|name| name == &sym.name);

                    let method_selected = class_def
                        .methods
                        .iter()
                        .any(|method| names.iter().any(|name| name == &method.id.name));

                    if class_selected || method_selected {
                        // If the class itself was selected, expose its method terms too.
                        // If only one method was selected, keep the owning class definition
                        // internally, but do not implicitly expose the rest of the methods.
                        selected_classes.push((*sym, class_selected));
                    }
                }

                for (sym, include_methods) in selected_classes {
                    self.include_class_with_superclasses(&mut out, &sym);

                    if include_methods && let Some(class_def) = self.class_defs.get(&sym) {
                        self.include_class_methods(&mut out, class_def);
                    }
                }

                // Importing a module makes its instances available. The explicit
                // import list controls names, not instance availability.
                out.instance_defs = self.instance_defs.clone();

                out
            }
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

        out.instance_defs.extend(self.instance_defs.clone());
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
    pub instance_defs: &'a mut Vec<
        solveig_ast::ast::InstanceDefinition<
            solveig_check::inference::type_infer::TcMetadata,
            std::ops::Range<usize>,
        >,
    >,
}

pub type SharedModuleApi = Arc<ModuleApi>;
pub type SharedTypedModuleApi = Arc<TypedModuleApi>;

#[derive(Debug, Clone)]
pub struct Imported<T> {
    pub alias: Symbol,
    pub module: ModuleName,
    pub api: Arc<T>,
    pub spec: ImportSpec,
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
        "pureIO" => "Creates an IO action that immediately returns a value.",
        "bindIO" => "Sequences an IO action and passes its result to the next action.",
        "error" => "Raises a defect from pure code.",
        "catchDefect" => "Handles a defect raised inside an installed IO scope.",
        "printLine" => "Writes text followed by a newline.",
        "printStr" => "Writes text without adding a newline.",
        "readLine" => "Reads one line of input.",
        "getArgs" => "Reads the command-line arguments passed to the program.",
        "readFile" => "Reads a file as text.",
        "writeFile" => "Writes text to a file, replacing its previous contents.",
        "appendFile" => "Appends text to the end of a file.",
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

fn compiler_type_entries() -> [(&'static str, TyConId); 8] {
    [
        ("Int", PRIM_TYS.int),
        ("Float", PRIM_TYS.float),
        ("Char", PRIM_TYS.char),
        ("String", PRIM_TYS.string),
        ("IO", PRIM_TYS.io),
        ("Bool", PRIM_TYS.bool),
        ("ThreadId", PRIM_TYS.thread_id),
        ("Chan", PRIM_TYS.chan),
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
