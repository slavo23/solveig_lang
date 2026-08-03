struct FrontendState<'a> {
    session: &'a Session,
    names: &'a mut Names,
    project: &'a Project,
    parsed: &'a ParsedProject,
    builtins: Arc<BuiltinApi>,
    apis: FxHashMap<ModuleName, SharedModuleApi>,
    typed_apis: FxHashMap<ModuleName, SharedTypedModuleApi>,
    resolutions: FxHashMap<ModuleId, Vec<(FileSpan, DefId)>>,
    resolved_type_aliases: FxHashMap<ModuleId, FxHashMap<DefId, TypeAlias>>,
    checked: FxHashMap<ModuleId, CheckedModule>,
    module_files: FxHashMap<ModuleId, FileId>,
    next_instance_id: u32,
    verify_core: bool,
    prelude_cache: Option<PreludeCache>,
    resolve_errors: Vec<ModuleResolveErrors>,
    type_errors: Vec<TypeError>,
    holes: Vec<HoleInfo>,
    non_reusable_modules: FxHashSet<ModuleName>,
    freshly_checked_modules: Vec<ModuleName>,
    reused_modules: Vec<ModuleName>,
}

type FrontendCaches<'cache> = (
    Option<&'cache PreludeCache>,
    Option<&'cache IncrementalCache>,
    &'cache FxHashSet<ModuleName>,
);
type FrontendProjectInput<'a> = (&'a Project, &'a ParsedProject, &'a [ModuleName]);
type FrontendRetained = (
    CompilationSnapshot,
    Option<PreludeCache>,
    Option<IncrementalCache>,
    Vec<String>,
    Vec<String>,
);

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

struct ModuleCheck {
    module: CheckedModule,
    failure: Option<FrontendPhase>,
}

impl<'a> FrontendState<'a> {
    fn new(
        session: &'a Session,
        names: &'a mut Names,
        project_input: FrontendProjectInput<'a>,
        verify_core: bool,
        caches: FrontendCaches<'_>,
    ) -> Result<Self, Vec<ResolveError>> {
        let (project, parsed, order) = project_input;
        let mut apis = FxHashMap::default();
        let mut errors = Vec::new();
        let (prelude_cache, incremental_cache, reusable_modules) = caches;
        let incremental_cache = incremental_cache.filter(|_| !reusable_modules.is_empty());
        if let Some(cache) = incremental_cache {
            *names = cache.names.clone();
        } else if let Some(cache) = prelude_cache {
            *names = cache.names.clone();
        }
        let builtins = prelude_cache.map_or_else(
            || Arc::new(driver_builtin_api(session, names, &project.module_table)),
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

            if reusable_modules.contains(module_name)
                && let Some(cache) = incremental_cache
            {
                let api = cache
                    .apis
                    .get(module_name)
                    .unwrap_or_else(|| {
                        panic!(
                            "ICE: reusable cache missing API for {}",
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
                    apis.insert(module_name.clone(), Arc::new(api));
                }
                Err(error) => errors.push(error),
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }
        errors.extend(validate_selected_import_names(project, &apis));
        if !errors.is_empty() {
            return Err(errors);
        }

        let mut typed_apis = prelude_cache
            .map(|cache| cache.typed_apis.clone())
            .unwrap_or_default();
        let mut checked = prelude_cache
            .map(|cache| cache.checked.clone())
            .unwrap_or_default();
        let mut module_files = prelude_cache
            .map(|cache| cache.module_files.clone())
            .unwrap_or_default();
        let mut resolutions = prelude_cache
            .map(|cache| cache.resolutions.clone())
            .unwrap_or_default();
        let mut next_instance_id = prelude_cache.map_or(0, |cache| cache.next_instance_id);

        if let Some(cache) = prelude_cache {
            rebase_cached_prelude_artifacts(project, cache, &mut module_files, &mut resolutions);
        }

        if let Some(cache) = incremental_cache {
            next_instance_id = next_instance_id.max(cache.next_instance_id);

            for module_name in reusable_modules {
                let module_id = project
                    .module_table
                    .id(module_name)
                    .expect("ICE: reusable module missing from current module table");
                typed_apis.insert(
                    module_name.clone(),
                    cache
                        .typed_apis
                        .get(module_name)
                        .unwrap_or_else(|| {
                            panic!(
                                "ICE: reusable cache missing typed API for {}",
                                module_name.as_string(session)
                            )
                        })
                        .clone(),
                );
                checked.insert(
                    module_id,
                    cache
                        .checked
                        .get(&module_id)
                        .unwrap_or_else(|| {
                            panic!(
                                "ICE: reusable cache missing checked module {}",
                                module_name.as_string(session)
                            )
                        })
                        .clone(),
                );
                let current_file = project
                    .modules
                    .get(module_name)
                    .unwrap_or_else(|| {
                        panic!(
                            "ICE: reusable module missing project file {}",
                            module_name.as_string(session)
                        )
                    })
                    .parsed
                    .file;
                module_files.insert(
                    module_id,
                    current_file,
                );
                resolutions.insert(
                    module_id,
                    rebase_resolutions_file(
                        cache.resolutions.get(&module_id).unwrap_or_else(|| {
                            panic!(
                                "ICE: reusable cache missing resolutions for {}",
                                module_name.as_string(session)
                            )
                        }),
                        current_file,
                    ),
                );
            }
        }

        Ok(Self {
            session,
            names,
            project,
            parsed,
            builtins,
            apis,
            typed_apis,
            resolutions,
            resolved_type_aliases: FxHashMap::default(),
            checked,
            module_files,
            next_instance_id,
            verify_core,
            prelude_cache: prelude_cache.cloned(),
            resolve_errors: Vec::new(),
            type_errors: Vec::new(),
            holes: Vec::new(),
            non_reusable_modules: FxHashSet::default(),
            freshly_checked_modules: Vec::new(),
            reused_modules: Vec::new(),
        })
    }

    fn into_snapshot(
        self,
        source_root: PathBuf,
        source_files: Vec<SourceFile>,
        order: &[ModuleName],
    ) -> FrontendRetained {
        let modules: Vec<CheckedModule> = order
            .iter()
            .filter_map(|module_name| {
                let module_id = self.project.module_table.id(module_name)?;
                self.checked.get(&module_id).cloned()
            })
            .collect();
        let incremental_cache = self.capture_incremental_cache();
        let freshly_checked_modules =
            module_names_as_strings(self.session, &self.freshly_checked_modules);
        let reused_modules = module_names_as_strings(self.session, &self.reused_modules);
        let mut resolutions = self.resolutions;
        for entries in resolutions.values_mut() {
            entries.sort_by_key(|(span, def)| {
                (
                    span.file,
                    span.span.start,
                    span.span.end,
                    def.module,
                    def.name,
                )
            });
            entries.dedup();
        }

        (
            CompilationSnapshot {
                source_root,
                project: self.project.clone(),
                parsed: self.parsed.clone(),
                names: self.names.clone(),
                apis: self.apis,
                typed_apis: self.typed_apis,
                modules,
                checked: self.checked,
                module_files: self.module_files,
                source_files,
                resolutions,
                resolve_errors: self.resolve_errors,
                type_errors: self.type_errors,
                holes: self.holes,
            },
            self.prelude_cache,
            Some(incremental_cache),
            freshly_checked_modules,
            reused_modules,
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
                self.reused_modules.push(module_name.clone());
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

            self.freshly_checked_modules.push(module_name.clone());
            let module_check = match self.check_module(module_name) {
                Ok(module_check) => module_check,
                Err(phase) => {
                    failed_modules.insert(module_name.clone());
                    first_failure = Some(first_failure.map_or(phase, |current| current.min(phase)));
                    continue;
                }
            };
            let checked = module_check.module;

            let typed_api = Arc::new(build_typed_module_api(
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
            if is_compiler_prelude_module(self.session, module_name) {
                self.capture_prelude_cache(order);
            }
            if let Some(phase) = module_check.failure {
                first_failure = Some(first_failure.map_or(phase, |current| current.min(phase)));
            }
        }

        first_failure
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

    fn check_module(&mut self, module_name: &ModuleName) -> Result<ModuleCheck, FrontendPhase> {
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
                    self.resolve_errors.push(ModuleResolveErrors {
                        module: module_name.clone(),
                        errors: errors.clone(),
                    });
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

        let mut check_env = TypeCheckEnv::from_module(&resolved);

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

        let checked_raw = match check_and_elab_module(&resolved, &check_env, self.names, file) {
            Ok(c) => c,
            Err(errors) => {
                self.type_errors.extend(errors.clone());
                for error in errors {
                    self.session.diagnostics().push(error.into_diagnostic(()));
                }
                return Err(FrontendPhase::Type);
            }
        };
        checked_raw.name_scratch.merge_into(self.names);

        for diagnostic in &checked_raw.diagnostics {
            self.session
                .diagnostics()
                .push(diagnostic.into_diagnostic(()));
        }

        let mut failure = None;
        if !checked_raw.holes.is_empty() {
            self.holes.extend(checked_raw.holes.clone());
            let diagnostics = hole_diagnostics(
                &checked_raw.holes,
                &check_env.constructor_types,
                &check_env.ctor_table,
                check_env.pretty_tables(self.names),
            );

            for diagnostic in diagnostics {
                self.session.diagnostics().push(diagnostic);
            }

            self.non_reusable_modules.insert(module_name.clone());
            failure = Some(FrontendPhase::Hole);
        }

        if self.verify_core && failure.is_none() {
            let lint_env =
                CoreLintEnv::from_typecheck_env(&checked_raw.module, &check_env, self.names)
                    .with_file(module_id, file);
            let diagnostics = lint_checked_module(&checked_raw.module, &lint_env);

            if !diagnostics.is_empty() {
                for diagnostic in diagnostics {
                    self.session.diagnostics().push(diagnostic);
                }
                return Err(FrontendPhase::Core);
            }
        }

        Ok(ModuleCheck {
            module: checked_raw.module,
            failure,
        })
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
                api: Arc::new(self.apis[&imp.module].filtered(&imp.spec)),
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

fn module_names_as_strings(session: &Session, modules: &[ModuleName]) -> Vec<String> {
    modules
        .iter()
        .map(|module| module.as_string(session))
        .collect()
}

fn validate_selected_import_names(
    project: &Project,
    apis: &FxHashMap<ModuleName, SharedModuleApi>,
) -> Vec<ResolveError> {
    let mut errors = Vec::new();

    for module_file in project.modules.values() {
        for import in &module_file.imports {
            let ImportSpec::Only(_) = &import.spec else {
                continue;
            };
            let Some(api) = apis.get(&import.module) else {
                continue;
            };

            for selected_name in &import.selected_names {
                if !api.exports_name(selected_name.0) {
                    errors.push(ResolveError::UnknownImportName {
                        span: FileSpan::new(module_file.parsed.file, selected_name.1),
                        module: import.module.clone(),
                        name: selected_name.0,
                    });
                }
            }
        }
    }

    errors
}

fn rebase_cached_prelude_artifacts(
    project: &Project,
    cache: &PreludeCache,
    module_files: &mut FxHashMap<ModuleId, FileId>,
    resolutions: &mut FxHashMap<ModuleId, Vec<(FileSpan, DefId)>>,
) {
    for (module_name, module_id) in &cache.module_ids {
        let Some(module_file) = project.modules.get(module_name) else {
            continue;
        };
        let current_file = module_file.parsed.file;

        module_files.insert(*module_id, current_file);
        if let Some(cached_resolutions) = cache.resolutions.get(module_id) {
            resolutions.insert(
                *module_id,
                rebase_resolutions_file(cached_resolutions, current_file),
            );
        }
    }
}

include!("frontend/imported_type_info.rs");
