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
    resolved: FxHashMap<ModuleId, solveig_ast::ast::ResolvedModule<Range<usize>>>,
    instance_defs: Vec<solveig_ast::ast::InstanceDefinition<Range<usize>>>,
    checked: FxHashMap<ModuleId, CheckedModule>,
    module_files: FxHashMap<ModuleId, FileId>,
    next_instance_id: u32,
    verify_core: bool,
    resolve_errors: Vec<ModuleResolveErrors>,
    type_errors: Vec<TypeError>,
    holes: Vec<HoleInfo>,
    non_reusable_modules: FxHashSet<ModuleName>,
    freshly_checked_modules: Vec<ModuleName>,
    reused_modules: Vec<ModuleName>,
}

type FrontendCaches<'cache> = (
    Option<&'cache PackageCache>,
    &'cache FxHashSet<ModuleName>,
);
type FrontendProjectInput<'a> = (&'a Project, &'a ParsedProject, &'a [ModuleName]);
type FrontendRetained = (
    CompilationSnapshot,
    Option<PackageCache>,
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
        let (package_cache, reusable_modules) = caches;
        let package_cache = package_cache.filter(|_| !reusable_modules.is_empty());
        if let Some(cache) = package_cache {
            *names = cache.names.clone();
        }
        let builtins = Arc::new(driver_builtin_api(session, names, &project.module_table));

        for module_name in order {
            if reusable_modules.contains(module_name)
                && let Some(cache) = package_cache
            {
                let api = cache
                    .apis
                    .get(module_name)
                    .unwrap_or_else(|| {
                        solveig_base::ice!(
                            "reusable cache missing API for {}",
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
            let package = project.module_table.package(module_id);
            match build_module_api(package, module_id, parsed.get(session, module_name)) {
                Ok(mut api) => {
                    seed_compiler_module_api(
                        session,
                        module_name,
                        module_id,
                        &builtins,
                        &mut api,
                    );
                    let parsed_module = parsed.get(session, module_name);
                    if let Err(error) = apply_module_visibility(parsed_module, &mut api)
                        .and_then(|()| apply_module_reexports(parsed_module, &mut api, &apis))
                    {
                        errors.push(error);
                    } else {
                        apis.insert(module_name.clone(), Arc::new(api));
                    }
                }
                Err(error) => errors.push(error),
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }
        let mut typed_apis = FxHashMap::default();
        let mut checked = FxHashMap::default();
        let mut module_files = FxHashMap::default();
        let mut resolutions = FxHashMap::default();
        let mut next_instance_id = 0;

        if let Some(cache) = package_cache {
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
                            solveig_base::ice!(
                                "reusable cache missing typed API for {}",
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
                            solveig_base::ice!(
                                "reusable cache missing checked module {}",
                                module_name.as_string(session)
                            )
                        })
                        .clone(),
                );
                let current_file = project
                    .modules
                    .get(module_name)
                    .unwrap_or_else(|| {
                        solveig_base::ice!(
                            "reusable module missing project file {}",
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
                            solveig_base::ice!(
                                "reusable cache missing resolutions for {}",
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
            resolved: FxHashMap::default(),
            instance_defs: Vec::new(),
            checked,
            module_files,
            next_instance_id,
            verify_core,
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
        let package_cache = self.capture_package_cache();
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
            Some(package_cache),
            freshly_checked_modules,
            reused_modules,
        )
    }

    fn resolve_all(&mut self, order: &[ModuleName]) -> Option<FrontendPhase> {
        let mut failed_modules = FxHashSet::default();
        let mut first_failure: Option<FrontendPhase> = None;

        for module_name in order {
            let Some(module_id) = self.project.module_table.id(module_name) else {
                solveig_base::ice!("resolved module order referenced unknown module");
            };
            if let Some(checked) = self.checked.get(&module_id) {
                let mut expr_anno = |metadata: TcMetadata| metadata.span;
                let mut pattern_anno = |span: Range<usize>| span;
                self.resolved.insert(
                    module_id,
                    checked
                        .clone()
                        .map_anno(&mut expr_anno, &mut pattern_anno),
                );
                continue;
            }

            let parsed = self.parsed.get(self.session, module_name);
            let imports =
                effective_imports(self.project, module_name, &parsed.imports, parsed.file);
            if imports
                .iter()
                .any(|import| failed_modules.contains(&import.module))
                || parsed
                    .directives
                    .reexports
                    .iter()
                    .any(|reexport| failed_modules.contains(&reexport.module))
            {
                failed_modules.insert(module_name.clone());
                continue;
            }

            if let Err(phase) = self.resolve_module(module_name) {
                failed_modules.insert(module_name.clone());
                first_failure = Some(first_failure.map_or(phase, |current| current.min(phase)));
            }
        }

        self.rebuild_instance_defs(order);

        first_failure
    }

    fn check_all(&mut self, order: &[ModuleName]) -> Option<FrontendPhase> {
        let mut failed_modules = FxHashSet::default();
        let mut first_failure: Option<FrontendPhase> = None;

        for module_name in order {
            let Some(module_id) = self.project.module_table.id(module_name) else {
                solveig_base::ice!("checked module order referenced unknown module");
            };
            if self.checked.contains_key(&module_id) {
                self.reused_modules.push(module_name.clone());
                continue;
            }
            if !self.resolved.contains_key(&module_id) {
                failed_modules.insert(module_name.clone());
                continue;
            }

            let parsed = self.parsed.get(self.session, module_name);
            let imports =
                effective_imports(self.project, module_name, &parsed.imports, parsed.file);

            if imports
                .iter()
                .any(|import| failed_modules.contains(&import.module))
                || parsed
                    .directives
                    .reexports
                    .iter()
                    .any(|reexport| failed_modules.contains(&reexport.module))
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
            let module_api = self.apis.get(module_name).unwrap_or_else(|| {
                solveig_base::ice!(
                    "module API missing during typed API construction for {}",
                    module_name.as_string(self.session)
                )
            });

            let mut typed_api = build_typed_module_api(
                self.session,
                self.names,
                module_id,
                module_name,
                &checked,
                self.resolved_type_aliases
                    .remove(&module_id)
                    .unwrap_or_default(),
                module_api,
            );
            typed_api.include_reexports(
                module_api,
                parsed
                    .directives
                    .reexports
                    .iter()
                    .filter_map(|reexport| self.typed_apis.get(&reexport.module))
                    .map(Arc::as_ref),
            );
            let typed_api = Arc::new(typed_api);

            self.typed_apis.insert(module_name.clone(), typed_api);
            self.module_files.insert(module_id, parsed.file);
            self.checked.insert(module_id, checked.clone());
            if let Some(phase) = module_check.failure {
                first_failure = Some(first_failure.map_or(phase, |current| current.min(phase)));
            }
        }

        first_failure
    }

    fn collect_imported_class_param_kind_ids(
        &self,
        imports: &[EffectiveImport],
        package: PackageId,
    ) -> FxHashMap<ClassId, Vec<solveig_ast::ast::Kind>> {
        let mut out = self
            .resolved
            .values()
            .flat_map(|module| &module.class_defs)
            .map(|class_def| {
                (
                    class_def.id,
                    class_def
                        .params
                        .iter()
                        .map(|(_, kind)| kind.clone())
                        .collect(),
                )
            })
            .collect::<FxHashMap<_, _>>();

        for imp in imports {
            if let Some(api) = self.typed_apis.get(&imp.module) {
                let module_api = self.apis.get(&imp.module).unwrap_or_else(|| {
                    solveig_base::ice!("typed imported API missing its module API")
                });
                let visible = api.visible_to(module_api, package);
                for class_def in visible.class_defs.values() {
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

    fn resolve_module(&mut self, module_name: &ModuleName) -> Result<(), FrontendPhase> {
        let parsed = self.parsed.get(self.session, module_name);
        let file = parsed.file;
        let module_id = self
            .project
            .module_table
            .id(module_name)
            .expect("ICE: module resolution referenced unknown module");
        let package = self.project.module_table.package(module_id);
        let imports = effective_imports(self.project, module_name, &parsed.imports, file);
        let imported_apis = match self.build_imported_apis(&imports, package) {
            Ok(imported) => imported,
            Err(errors) => {
                self.resolve_errors.push(ModuleResolveErrors {
                    module: module_name.clone(),
                    errors: errors.clone(),
                });
                push_resolve_errors(self.session, Some(&self.project.module_table), errors);
                return Err(FrontendPhase::Resolve);
            }
        };
        let local_api = self.apis.get(module_name).unwrap_or_else(|| {
            solveig_base::ice!(
                "module API missing during resolve for {}",
                module_name.as_string(self.session)
            )
        });
        let cx = ModuleContext {
            module: module_name.clone(),
            module_id,
            file,
            local_api: Arc::new(local_api.local_view()),
            imported_apis,
            builtins: self.builtins.clone(),
        };
        let resolver_imports = ResolverImports {
            tycon_kind_ids: self.collect_imported_kind_ids(&imports, package),
            class_param_kind_ids: self.collect_imported_class_param_kind_ids(&imports, package),
            type_aliases: self.collect_imported_type_aliases(&imports, package),
        };

        let resolver = Resolver::new(self.session, self.names, &self.project.module_table);
        let mut resolved_info =
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
        resolved_info
            .resolutions
            .extend(self.module_directive_resolutions(parsed, local_api, package));
        self.resolutions
            .insert(module_id, resolved_info.resolutions);
        self.resolved_type_aliases
            .insert(module_id, resolved_info.type_aliases);
        self.stamp_instance_ids(&mut resolved_info.module);
        self.resolved.insert(module_id, resolved_info.module);

        Ok(())
    }

    fn check_module(&mut self, module_name: &ModuleName) -> Result<ModuleCheck, FrontendPhase> {
        let parsed = self.parsed.get(self.session, module_name);
        let file = parsed.file;
        let module_id = self
            .project
            .module_table
            .id(module_name)
            .expect("ICE: module check referenced unknown module");
        let package = self.project.module_table.package(module_id);
        let imports = effective_imports(self.project, module_name, &parsed.imports, file);

        let imported_apis = match self.build_imported_apis(&imports, package) {
            Ok(imported) => imported,
            Err(errors) => {
                self.resolve_errors.push(ModuleResolveErrors {
                    module: module_name.clone(),
                    errors: errors.clone(),
                });
                push_resolve_errors(self.session, Some(&self.project.module_table), errors);
                return Err(FrontendPhase::Resolve);
            }
        };
        let imported_typed = self.build_imported_typed_apis(&imports, &imported_apis, package);
        let resolved = self.resolved.get(&module_id).unwrap_or_else(|| {
            solveig_base::ice!(
                "resolved module missing during typecheck for {}",
                module_name.as_string(self.session)
            )
        });

        let ImportedTypeInfo {
            func_schemes,
            tycon_kinds,
            constructor_types,
            class_defs: imported_class_defs,
            tycon_table: imported_tycon_table,
            ctor_table: imported_ctor_table,
            class_table: imported_class_table,
        } = collect_imported_type_info(&imported_typed);

        let mut check_env = TypeCheckEnv::from_module(resolved);

        check_env.hole_constructor_ids.extend(
            imported_typed
                .iter()
                .flat_map(|imported| imported.unqualified.constructor_ids.values())
                .copied(),
        );
        check_env.hole_constructor_ids.sort();
        check_env.hole_constructor_ids.dedup();

        check_env.func_schemes.extend(func_schemes);
        check_env.tycon_kinds.extend(tycon_kinds);
        check_env.constructor_types.extend(constructor_types);
        check_env.class_defs.extend(imported_class_defs);
        self.extend_instance_support(&mut check_env);
        check_env.instance_defs.clone_from(&self.instance_defs);
        for (id, info) in imported_tycon_table.iter() {
            check_env.tycon_table.insert_with_id(id, info.clone());
        }
        for (id, info) in imported_ctor_table.iter() {
            check_env.ctor_table.insert_with_id(id, info.clone());
        }
        for (id, info) in imported_class_table.iter() {
            check_env.class_table.insert_with_id(id, info.clone());
        }
        let checked_raw = match check_and_elab_module(resolved, &check_env, self.names, file) {
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

}

fn module_names_as_strings(session: &Session, modules: &[ModuleName]) -> Vec<String> {
    modules
        .iter()
        .map(|module| module.as_string(session))
        .collect()
}

include!("frontend/imported_type_info.rs");
include!("frontend/imports.rs");
include!("frontend/instances.rs");
