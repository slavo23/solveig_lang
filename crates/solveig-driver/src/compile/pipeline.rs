//! Frontend orchestration from project loading through checked modules.
//! The pipeline accumulates diagnostics and retains per-module checked artifacts.

use super::*;

pub(super) struct CheckProjectOutcome {
    pub(super) checked_project: Option<CheckedProject>,
    pub(super) phase: Option<FrontendPhase>,
}

pub(super) fn verify_lowering_core(
    merged: &CheckedModule,
    checked_project: &CheckedProject,
) -> Vec<Diagnostic> {
    let lint_env = CoreLintEnv::from_module(merged, &checked_project.names)
        .with_files(checked_project.module_files.clone());
    lint_checked_module(merged, &lint_env)
}

impl Compilation {
    pub(super) fn check_project_modules(
        &mut self,
        verify_core: bool,
    ) -> Result<CheckProjectOutcome, DriverError> {
        let entry_file = find_entry_file(&self.project_root)?;
        let (_root, project) = match load_project(&self.session, &entry_file) {
            Ok(project) => project,
            Err(ProjectError::Driver(error)) => return Err(error),
            Err(ProjectError::Resolve(error)) => {
                push_resolve_errors(&self.session, None, vec![error]);
                return Ok(CheckProjectOutcome {
                    checked_project: None,
                    phase: Some(FrontendPhase::Resolve),
                });
            }
        };

        if self.session.diagnostics().has_errors() {
            return Ok(CheckProjectOutcome {
                checked_project: None,
                phase: Some(FrontendPhase::Parse),
            });
        }

        let order = match module_order_checked(&self.session, &project) {
            Ok(order) => order,
            Err(error) => {
                push_resolve_errors(&self.session, Some(&project.module_table), vec![error]);
                return Ok(CheckProjectOutcome {
                    checked_project: None,
                    phase: Some(FrontendPhase::Resolve),
                });
            }
        };
        let parsed = parse_project(&project);

        let mut state = match FrontendState::new(
            &self.session,
            &mut self.names,
            &project,
            &parsed,
            &order,
            verify_core,
        ) {
            Ok(state) => state,
            Err(errors) => {
                push_resolve_errors(&self.session, Some(&project.module_table), errors);
                return Ok(CheckProjectOutcome {
                    checked_project: None,
                    phase: Some(FrontendPhase::Resolve),
                });
            }
        };
        let phase = state.check_all(&order);

        let source_files = self.session.source_map().files().to_vec();
        Ok(CheckProjectOutcome {
            checked_project: Some(state.into_checked_project(source_files)),
            phase,
        })
    }
}

fn push_resolve_errors(
    session: &Session,
    modules: Option<&ModuleTable>,
    errors: Vec<ResolveError>,
) {
    for error in errors {
        session
            .diagnostics()
            .push(error.into_diagnostic((session, modules)));
    }
}

struct FrontendState<'a> {
    session: &'a Session,
    names: &'a mut Names,
    project: &'a Project,
    parsed: &'a ParsedProject,
    builtins: Rc<BuiltinApi>,
    apis: HashMap<ModuleName, SharedModuleApi>,
    typed_apis: HashMap<ModuleName, SharedTypedModuleApi>,
    resolutions: HashMap<ModuleId, Vec<(FileSpan, DefId)>>,
    checked_modules: Vec<CheckedModule>,
    checked: HashMap<ModuleId, CheckedModule>,
    module_files: HashMap<ModuleId, FileId>,
    next_instance_id: u32,
    verify_core: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum FrontendPhase {
    Parse,
    Resolve,
    Kind,
    Type,
    Hole,
    Core,
}

impl FrontendPhase {
    pub(super) fn error_message(self) -> &'static str {
        match self {
            Self::Parse => "parse error",
            Self::Resolve => "resolve error",
            Self::Kind => "kind error",
            Self::Type => "type error",
            Self::Hole => "Program has unfilled holes. Aborting compilation",
            Self::Core => "core verification failed",
        }
    }
}

impl<'a> FrontendState<'a> {
    fn new(
        session: &'a Session,
        names: &'a mut Names,
        project: &'a Project,
        parsed: &'a ParsedProject,
        order: &[ModuleName],
        verify_core: bool,
    ) -> Result<Self, Vec<ResolveError>> {
        let mut apis = HashMap::new();
        let mut errors = Vec::new();
        let builtins = Rc::new(driver_builtin_api(session, names, &project.module_table));

        for module_name in order {
            let module_id = project
                .module_table
                .id(module_name)
                .expect("ICE: checked module order referenced unknown module");
            match build_module_api(module_id, parsed.get(session, module_name)) {
                Ok(mut api) => {
                    seed_compiler_prelude_module_api(
                        session,
                        module_name,
                        module_id,
                        &builtins,
                        &mut api,
                    );
                    apis.insert(module_name.clone(), Rc::new(api));
                }
                Err(error) => errors.push(error),
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(Self {
            session,
            names,
            project,
            parsed,
            builtins,
            apis,
            typed_apis: HashMap::new(),
            resolutions: HashMap::new(),
            checked_modules: Vec::new(),
            checked: HashMap::new(),
            module_files: HashMap::new(),
            next_instance_id: 0,
            verify_core,
        })
    }

    fn into_checked_project(self, source_files: Vec<SourceFile>) -> CheckedProject {
        CheckedProject {
            names: self.names.clone(),
            modules: self.checked_modules,
            checked: self.checked,
            module_files: self.module_files,
            source_files,
        }
    }

    fn check_all(&mut self, order: &[ModuleName]) -> Option<FrontendPhase> {
        let mut failed_modules = HashSet::new();
        let mut first_failure: Option<FrontendPhase> = None;

        for module_name in order {
            let parsed = self.parsed.get(self.session, module_name);
            let imports =
                effective_imports(self.session, module_name, &parsed.imports, parsed.file);

            if imports
                .iter()
                .any(|import| failed_modules.contains(&import.module))
            {
                failed_modules.insert(module_name.clone());
                continue;
            }

            let checked = match self.check_module(module_name) {
                Ok(checked) => checked,
                Err(phase) => {
                    failed_modules.insert(module_name.clone());
                    first_failure = Some(first_failure.map_or(phase, |current| current.min(phase)));
                    continue;
                }
            };

            let module_id = self
                .project
                .module_table
                .id(module_name)
                .expect("ICE: checked module order referenced unknown module");
            let typed_api = Rc::new(build_typed_module_api(
                self.session,
                self.names,
                module_id,
                module_name,
                &checked,
            ));

            self.typed_apis.insert(module_name.clone(), typed_api);
            self.module_files.insert(module_id, parsed.file);
            self.checked.insert(module_id, checked.clone());
            self.checked_modules.push(checked);
        }

        first_failure
    }

    fn collect_imported_class_param_kind_ids(
        &self,
        imports: &[EffectiveImport],
    ) -> HashMap<ClassId, Vec<solveig_ast::ast::Kind>> {
        let mut out = HashMap::new();

        for imp in imports {
            if let Some(api) = self.typed_apis.get(&imp.module) {
                for class_def in api.filtered(&imp.spec).class_defs.values() {
                    out.insert(
                        class_def.id,
                        class_def
                            .params
                            .iter()
                            .map(|(_, kind)| kind.clone())
                            .collect(),
                    );
                }
            }
        }

        out
    }

    fn check_module(&mut self, module_name: &ModuleName) -> Result<CheckedModule, FrontendPhase> {
        let parsed = self.parsed.get(self.session, module_name);
        let file = parsed.file;
        let module_id = self
            .project
            .module_table
            .id(module_name)
            .expect("ICE: module check referenced unknown module");
        let imports = effective_imports(self.session, module_name, &parsed.imports, file);

        let imported_apis = self.build_imported_apis(&imports);
        let cx = ModuleContext {
            module: module_name.clone(),
            module_id,
            file,
            local_api: self.apis[module_name].clone(),
            imported_apis,
            builtins: self.builtins.clone(),
        };

        let resolver_imports = ResolverImports {
            tycon_kind_ids: self.collect_imported_kind_ids(&imports),
            class_param_kind_ids: self.collect_imported_class_param_kind_ids(&imports),
        };

        let resolver = Resolver::new(self.session, self.names, &self.project.module_table);
        let resolved_info =
            match resolver.resolve_module(module_name, parsed, &cx, &resolver_imports) {
                Ok(resolved) => resolved,
                Err(ResolverError::Resolve(errors)) => {
                    push_resolve_errors(self.session, Some(&self.project.module_table), errors);
                    return Err(FrontendPhase::Resolve);
                }
                Err(ResolverError::Kind(err)) => {
                    self.session.diagnostics().push(err.into_diagnostic(()));
                    return Err(FrontendPhase::Kind);
                }
            };
        self.resolutions
            .insert(module_id, resolved_info.resolutions.clone());
        let mut resolved = resolved_info.module;
        self.stamp_instance_ids(&mut resolved);

        let imported_typed = self.build_imported_typed_apis(&imports);
        let ImportedTypeInfo {
            func_schemes,
            tycon_kinds,
            constructor_types,
            class_defs: imported_class_defs,
            instance_defs: imported_instance_defs,
            tycon_table: imported_tycon_table,
            ctor_table: imported_ctor_table,
            class_table: imported_class_table,
        } = collect_imported_type_info(&imported_typed);

        let mut check_env = TypeCheckEnv::from_module(&resolved, self.names);

        check_env.func_schemes.extend(func_schemes);
        check_env.tycon_kinds.extend(tycon_kinds);
        check_env.constructor_types.extend(constructor_types);
        check_env.class_defs.extend(imported_class_defs);
        for (id, info) in imported_tycon_table.iter() {
            check_env.tycon_table.insert_with_id(id, info.clone());
        }
        for (id, info) in imported_ctor_table.iter() {
            check_env.ctor_table.insert_with_id(id, info.clone());
        }
        for (id, info) in imported_class_table.iter() {
            check_env.class_table.insert_with_id(id, info.clone());
        }
        check_env.instance_defs.extend(
            imported_instance_defs
                .into_iter()
                .map(|inst| inst.map_anno(&mut |meta| meta.span, &mut |span| span)),
        );

        let checked_raw = match check_and_elab_module(&resolved, &check_env, file) {
            Ok(c) => c,
            Err(errors) => {
                for error in errors {
                    self.session.diagnostics().push(error.into_diagnostic(()));
                }
                return Err(FrontendPhase::Type);
            }
        };
        *self.names = checked_raw.names.clone();
        check_env.names = checked_raw.names.clone();

        for diagnostic in &checked_raw.diagnostics {
            self.session
                .diagnostics()
                .push(diagnostic.into_diagnostic(()));
        }

        if !checked_raw.holes.is_empty() {
            let diagnostics = hole_diagnostics(
                &checked_raw.holes,
                &check_env.constructor_types,
                &check_env.ctor_table,
                check_env.pretty_tables(),
            );

            for diagnostic in diagnostics {
                self.session.diagnostics().push(diagnostic);
            }

            return Err(FrontendPhase::Hole);
        }

        if self.verify_core {
            let lint_env = CoreLintEnv::from_typecheck_env(&checked_raw.module, &check_env)
                .with_file(module_id, file);
            let diagnostics = lint_checked_module(&checked_raw.module, &lint_env);

            if !diagnostics.is_empty() {
                for diagnostic in diagnostics {
                    self.session.diagnostics().push(diagnostic);
                }
                return Err(FrontendPhase::Core);
            }
        }

        Ok(checked_raw.module)
    }

    fn stamp_instance_ids(&mut self, module: &mut solveig_ast::ast::ResolvedModule<Range<usize>>) {
        for inst in &mut module.instance_defs {
            let id = self.next_instance_id;
            self.next_instance_id = self
                .next_instance_id
                .checked_add(1)
                .expect("ICE: instance id space exhausted");
            inst.id = InstanceId(id);
        }
    }

    fn build_imported_apis(&self, imports: &[EffectiveImport]) -> Vec<ImportedApi> {
        imports
            .iter()
            .map(|imp| ImportedApi {
                alias: imp.alias,
                module: imp.module.clone(),
                api: Rc::new(self.apis[&imp.module].filtered(&imp.spec)),
                spec: imp.spec.clone(),
            })
            .collect()
    }

    fn build_imported_typed_apis(&self, imports: &[EffectiveImport]) -> Vec<ImportedTypedApi> {
        imports
            .iter()
            .filter_map(|imp| {
                self.typed_apis
                    .get(&imp.module)
                    .map(|api| ImportedTypedApi {
                        alias: imp.alias,
                        module: imp.module.clone(),
                        api: api.clone(),
                        spec: imp.spec.clone(),
                    })
            })
            .collect()
    }

    fn collect_imported_kind_ids(
        &self,
        imports: &[EffectiveImport],
    ) -> HashMap<TyConId, solveig_ast::ast::Kind> {
        let mut out = HashMap::new();

        for imp in imports {
            if let Some(api) = self.typed_apis.get(&imp.module) {
                let filtered = api.filtered(&imp.spec);
                for (sym, kind) in &filtered.type_kinds {
                    let id = filtered
                        .type_ids
                        .get(sym)
                        .copied()
                        .expect("ICE: imported type kind missing typed API TyConId");
                    out.insert(id, kind.clone());
                }
            }
        }

        out
    }
}

struct ImportedTypeInfo {
    func_schemes: HashMap<DefId, Scheme>,
    tycon_kinds: HashMap<TyConId, solveig_ast::ast::Kind>,
    constructor_types: HashMap<CtorId, solveig_ast::ast::Ty>,
    class_defs: HashMap<ClassId, ClassDefinition>,
    instance_defs: Vec<solveig_ast::ast::InstanceDefinition<TcMetadata, Range<usize>>>,
    tycon_table: TyConTable,
    ctor_table: CtorTable,
    class_table: ClassTable,
}

fn collect_imported_type_info(imports: &[ImportedTypedApi]) -> ImportedTypeInfo {
    let mut func_schemes = HashMap::new();
    let mut tycon_kinds = HashMap::new();
    let mut constructor_types = HashMap::new();
    let mut class_defs = HashMap::new();
    let mut instance_defs = Vec::new();
    let mut tycon_table = TyConTable::new();
    let mut ctor_table = CtorTable::new();
    let mut class_table = ClassTable::new();

    for imp in imports {
        let mut display_maps = TypedDisplayMaps {
            func_schemes: &mut func_schemes,
            tycon_kinds: &mut tycon_kinds,
            constructor_types: &mut constructor_types,
            class_defs: &mut class_defs,
            tycon_table: &mut tycon_table,
            ctor_table: &mut ctor_table,
            class_table: &mut class_table,
            instance_defs: &mut instance_defs,
        };

        imp.api
            .filtered(&imp.spec)
            .extend_display_maps(&mut display_maps);
    }

    ImportedTypeInfo {
        func_schemes,
        tycon_kinds,
        constructor_types,
        class_defs,
        instance_defs,
        tycon_table,
        ctor_table,
        class_table,
    }
}

fn find_entry_file(project_root: &Path) -> Result<PathBuf, DriverError> {
    let entry = project_root.join("src").join("main.sv");
    if entry.is_file() {
        Ok(entry)
    } else {
        Err(DriverError::EntryFileMissing { path: entry })
    }
}

pub(super) fn merge_checked_modules(
    mods: Vec<CheckedModule>,
    names: &mut Names,
) -> Result<CheckedModule, DriverError> {
    let mut data_defs = Vec::new();
    let mut class_defs = Vec::new();
    let mut instance_defs = Vec::new();
    let mut builtin_defs = Vec::new();
    let mut tycon_kinds = HashMap::new();
    let mut constructor_types = HashMap::new();
    let mut func_schemes = HashMap::new();
    let mut func_defs = Vec::new();

    for m in mods {
        let CheckedModule {
            data_defs: module_data_defs,
            class_defs: module_class_defs,
            instance_defs: module_instance_defs,
            builtin_defs: module_builtin_defs,
            tycon_table: module_tycon_table,
            ctor_table: module_ctor_table,
            class_table: _,
            tycon_kinds: module_tycon_kinds,
            constructor_types: module_constructor_types,
            func_schemes: module_func_schemes,
            func_defs: module_func_defs,
        } = m;

        data_defs.extend(module_data_defs);
        class_defs.extend(module_class_defs);
        instance_defs.extend(module_instance_defs);
        builtin_defs.extend(module_builtin_defs);
        extend_unique_tycon(
            &mut tycon_kinds,
            module_tycon_kinds,
            &module_tycon_table,
            names,
        )?;
        extend_unique_ctor(
            &mut constructor_types,
            module_constructor_types,
            &module_ctor_table,
            names,
        )?;
        extend_unique_def(&mut func_schemes, module_func_schemes, "function", names)?;
        func_defs.extend(module_func_defs);
    }

    let mut module = solveig_ast::ast::ResolvedModule {
        data_defs,
        class_defs,
        instance_defs,
        builtin_defs,
        tycon_table: Default::default(),
        ctor_table: Default::default(),
        class_table: Default::default(),
        tycon_kinds,
        constructor_types,
        func_schemes,
        func_defs,
    };
    module.rebuild_identity_tables(names);

    Ok(module)
}

fn extend_unique_def<T>(
    dst: &mut HashMap<DefId, T>,
    src: HashMap<DefId, T>,
    label: &'static str,
    names: &Names,
) -> Result<(), DriverError> {
    for (k, v) in src {
        if dst.insert(k, v).is_some() {
            return Err(DriverError::DuplicateDefinition {
                kind: label,
                name: names.def_display_name(k),
            });
        }
    }
    Ok(())
}

fn extend_unique_tycon(
    dst: &mut HashMap<TyConId, solveig_ast::ast::Kind>,
    src: HashMap<TyConId, solveig_ast::ast::Kind>,
    tycons: &TyConTable,
    names: &Names,
) -> Result<(), DriverError> {
    for (k, v) in src {
        if dst.insert(k, v).is_some() {
            return Err(DriverError::DuplicateDefinition {
                kind: "type constructor",
                name: tycon_name_for_error(k, tycons, names),
            });
        }
    }
    Ok(())
}

fn extend_unique_ctor(
    dst: &mut HashMap<CtorId, Ty>,
    src: HashMap<CtorId, Ty>,
    ctors: &CtorTable,
    names: &Names,
) -> Result<(), DriverError> {
    for (k, v) in src {
        if dst.insert(k, v).is_some() {
            return Err(DriverError::DuplicateDefinition {
                kind: "constructor",
                name: ctor_name_for_error(k, ctors, names),
            });
        }
    }
    Ok(())
}

fn tycon_name_for_error(id: TyConId, tycons: &TyConTable, names: &Names) -> String {
    tycons
        .get(id)
        .map(|info| names.symbol_display_name(info.name))
        .unwrap_or_else(|| format!("T{}", id.0))
}

fn ctor_name_for_error(id: CtorId, ctors: &CtorTable, names: &Names) -> String {
    ctors
        .get(id)
        .map(|info| names.symbol_display_name(info.name))
        .unwrap_or_else(|| format!("C{}", id.0))
}
