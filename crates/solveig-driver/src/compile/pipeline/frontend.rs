struct FrontendState<'a> {
    session: &'a Session,
    names: &'a mut Names,
    project: &'a Project,
    parsed: &'a ParsedProject,
    builtins: Rc<BuiltinApi>,
    apis: FxHashMap<ModuleName, SharedModuleApi>,
    typed_apis: FxHashMap<ModuleName, SharedTypedModuleApi>,
    resolutions: FxHashMap<ModuleId, Vec<(FileSpan, DefId)>>,
    resolved_type_aliases: FxHashMap<ModuleId, FxHashMap<DefId, TypeAlias>>,
    checked_modules: Vec<CheckedModule>,
    checked: FxHashMap<ModuleId, CheckedModule>,
    module_files: FxHashMap<ModuleId, FileId>,
    next_instance_id: u32,
    verify_core: bool,
    prelude_cache: Option<PreludeCache>,
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
        prelude_cache: Option<&PreludeCache>,
    ) -> Result<Self, Vec<ResolveError>> {
        let mut apis = FxHashMap::default();
        let mut errors = Vec::new();
        if let Some(cache) = prelude_cache {
            *names = cache.names.clone();
        }
        let builtins = prelude_cache.map_or_else(
            || Rc::new(driver_builtin_api(session, names, &project.module_table)),
            |cache| cache.builtins.clone(),
        );

        for module_name in order {
            if let Some(cache) = prelude_cache
                && is_compiler_prelude_module(session, module_name)
            {
                let api = cache
                    .apis
                    .get(module_name)
                    .unwrap_or_else(|| {
                        panic!(
                            "ICE: usable prelude cache missing API for {}",
                            module_name.as_string(session)
                        )
                    })
                    .clone();
                apis.insert(module_name.clone(), api);
                continue;
            }

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

        let typed_apis = prelude_cache
            .map(|cache| cache.typed_apis.clone())
            .unwrap_or_default();
        let checked_modules = prelude_cache
            .map(|cache| cache.checked_modules.clone())
            .unwrap_or_default();
        let checked = prelude_cache
            .map(|cache| cache.checked.clone())
            .unwrap_or_default();
        let module_files = prelude_cache
            .map(|cache| cache.module_files.clone())
            .unwrap_or_default();
        let next_instance_id = prelude_cache.map_or(0, |cache| cache.next_instance_id);

        Ok(Self {
            session,
            names,
            project,
            parsed,
            builtins,
            apis,
            typed_apis,
            resolutions: FxHashMap::default(),
            resolved_type_aliases: FxHashMap::default(),
            checked_modules,
            checked,
            module_files,
            next_instance_id,
            verify_core,
            prelude_cache: prelude_cache.cloned(),
        })
    }

    fn into_checked_project(
        self,
        source_files: Vec<SourceFile>,
    ) -> (CheckedProject, Option<PreludeCache>) {
        let docs = self
            .typed_apis
            .values()
            .flat_map(|api| api.docs.iter().map(|(id, doc)| (*id, doc.clone())))
            .collect();

        (
            CheckedProject {
                names: self.names.clone(),
                modules: self.checked_modules,
                checked: self.checked,
                module_files: self.module_files,
                source_files,
                docs,
            },
            self.prelude_cache,
        )
    }

    fn check_all(&mut self, order: &[ModuleName]) -> Option<FrontendPhase> {
        let mut failed_modules = FxHashSet::default();
        let mut first_failure: Option<FrontendPhase> = None;

        for module_name in order {
            let module_id = self
                .project
                .module_table
                .id(module_name)
                .expect("ICE: checked module order referenced unknown module");
            if self.checked.contains_key(&module_id) {
                continue;
            }

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

            let typed_api = Rc::new(build_typed_module_api(
                self.session,
                self.names,
                module_id,
                module_name,
                &checked,
                self.resolved_type_aliases
                    .remove(&module_id)
                    .unwrap_or_default(),
                self.apis[module_name].docs.clone(),
            ));

            self.typed_apis.insert(module_name.clone(), typed_api);
            self.module_files.insert(module_id, parsed.file);
            self.checked.insert(module_id, checked.clone());
            self.checked_modules.push(checked);
            if is_compiler_prelude_module(self.session, module_name) {
                self.capture_prelude_cache(order);
            }
        }

        first_failure
    }

    fn capture_prelude_cache(&mut self, order: &[ModuleName]) {
        let mut prelude_names = Vec::new();
        for module_name in order {
            if is_compiler_prelude_module(self.session, module_name) {
                prelude_names.push(module_name.clone());
            }
        }

        if prelude_names.iter().any(|module_name| {
            let module_id = self
                .project
                .module_table
                .id(module_name)
                .expect("ICE: checked module order referenced unknown module");
            !self.checked.contains_key(&module_id) || !self.typed_apis.contains_key(module_name)
        }) {
            return;
        }

        let mut project_modules = FxHashMap::default();
        let mut module_ids = FxHashMap::default();
        let mut apis = FxHashMap::default();
        let mut typed_apis = FxHashMap::default();
        let mut checked = FxHashMap::default();
        let mut module_files = FxHashMap::default();
        let mut checked_modules = Vec::new();

        for module_name in &prelude_names {
            let module_id = self
                .project
                .module_table
                .id(module_name)
                .expect("ICE: checked module order referenced unknown module");
            project_modules.insert(
                module_name.clone(),
                self.project
                    .modules
                    .get(module_name)
                    .unwrap_or_else(|| {
                        panic!(
                            "ICE: prelude cache missing project module {}",
                            module_name.as_string(self.session)
                        )
                    })
                    .clone(),
            );
            module_ids.insert(module_name.clone(), module_id);
            apis.insert(
                module_name.clone(),
                self.apis
                    .get(module_name)
                    .unwrap_or_else(|| {
                        panic!(
                            "ICE: prelude cache missing module API {}",
                            module_name.as_string(self.session)
                        )
                    })
                    .clone(),
            );
            typed_apis.insert(
                module_name.clone(),
                self.typed_apis
                    .get(module_name)
                    .unwrap_or_else(|| {
                        panic!(
                            "ICE: prelude cache missing typed API {}",
                            module_name.as_string(self.session)
                        )
                    })
                    .clone(),
            );
            checked.insert(
                module_id,
                self.checked
                    .get(&module_id)
                    .unwrap_or_else(|| {
                        panic!(
                            "ICE: prelude cache missing checked module {}",
                            module_name.as_string(self.session)
                        )
                    })
                    .clone(),
            );
            module_files.insert(
                module_id,
                *self.module_files.get(&module_id).unwrap_or_else(|| {
                    panic!(
                        "ICE: prelude cache missing file for {}",
                        module_name.as_string(self.session)
                    )
                }),
            );
        }

        for module_name in &prelude_names {
            let module_id = *module_ids.get(module_name).unwrap_or_else(|| {
                panic!(
                    "ICE: prelude cache missing module id for {}",
                    module_name.as_string(self.session)
                )
            });
            checked_modules.push(
                checked
                    .get(&module_id)
                    .unwrap_or_else(|| {
                        panic!(
                            "ICE: prelude cache missing checked module {}",
                            module_name.as_string(self.session)
                        )
                    })
                    .clone(),
            );
        }

        self.prelude_cache = Some(PreludeCache {
            project_modules,
            module_ids,
            names: self.names.clone(),
            builtins: self.builtins.clone(),
            apis,
            typed_apis,
            checked_modules,
            checked,
            module_files,
            next_instance_id: self.next_instance_id,
            verified_core: self.verify_core,
        });
    }

    fn collect_imported_class_param_kind_ids(
        &self,
        imports: &[EffectiveImport],
    ) -> FxHashMap<ClassId, Vec<solveig_ast::ast::Kind>> {
        let mut out = FxHashMap::default();

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
            type_aliases: self.collect_imported_type_aliases(&imports),
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
        self.resolved_type_aliases
            .insert(module_id, resolved_info.type_aliases.clone());
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
    ) -> FxHashMap<TyConId, solveig_ast::ast::Kind> {
        let mut out = FxHashMap::default();

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

    fn collect_imported_type_aliases(
        &self,
        imports: &[EffectiveImport],
    ) -> FxHashMap<DefId, TypeAlias> {
        let mut out = FxHashMap::default();

        for imp in imports {
            if let Some(api) = self.typed_apis.get(&imp.module) {
                out.extend(api.filtered(&imp.spec).type_aliases);
            }
        }

        out
    }
}

struct ImportedTypeInfo {
    func_schemes: FxHashMap<DefId, Scheme>,
    tycon_kinds: FxHashMap<TyConId, solveig_ast::ast::Kind>,
    constructor_types: FxHashMap<CtorId, solveig_ast::ast::Ty>,
    class_defs: FxHashMap<ClassId, ClassDefinition>,
    instance_defs: Vec<solveig_ast::ast::InstanceDefinition<TcMetadata, Range<usize>>>,
    tycon_table: TyConTable,
    ctor_table: CtorTable,
    class_table: ClassTable,
}

fn collect_imported_type_info(imports: &[ImportedTypedApi]) -> ImportedTypeInfo {
    let mut func_schemes = FxHashMap::default();
    let mut tycon_kinds = FxHashMap::default();
    let mut constructor_types = FxHashMap::default();
    let mut class_defs = FxHashMap::default();
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
