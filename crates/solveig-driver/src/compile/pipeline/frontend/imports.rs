// Import and module-directive projections used by frontend orchestration.

impl FrontendState<'_> {
    fn build_imported_apis(
        &self,
        imports: &[EffectiveImport],
        package: PackageId,
    ) -> Vec<ImportedApi> {
        imports
            .iter()
            .map(|import| ImportedApi {
                alias: import.alias,
                module: import.module.clone(),
                api: Arc::new(
                    self.apis
                        .get(&import.module)
                        .unwrap_or_else(|| {
                            solveig_base::ice!(
                                "imported module API missing for {}",
                                import.module.as_string(self.session)
                            )
                        })
                        .visible_to(package)
                        .filtered(&import.spec),
                ),
                spec: import.spec.clone(),
            })
            .collect()
    }

    fn build_imported_typed_apis(
        &self,
        imports: &[EffectiveImport],
        package: PackageId,
    ) -> Vec<ImportedTypedApi> {
        imports
            .iter()
            .filter_map(|import| {
                self.typed_apis.get(&import.module).map(|api| {
                    let module_api = self.apis.get(&import.module).unwrap_or_else(|| {
                        solveig_base::ice!("typed imported API missing its module API")
                    });
                    ImportedTypedApi {
                        alias: import.alias,
                        module: import.module.clone(),
                        api: Arc::new(api.imported_view(module_api, package, &import.spec)),
                        spec: ImportSpec::All,
                    }
                })
            })
            .collect()
    }

    fn module_directive_resolutions(
        &self,
        parsed: &ParsedModule,
        local_api: &ModuleApi,
        package: PackageId,
    ) -> Vec<(FileSpan, DefId)> {
        let mut resolutions = Vec::new();
        for directive in parsed
            .directives
            .exports
            .iter()
            .chain(&parsed.directives.private)
        {
            for entity in &directive.entities {
                if let Some(definition) = local_api.definition(entity.name.0) {
                    resolutions.push((FileSpan::new(parsed.file, entity.name.1), definition));
                    resolutions.push((
                        FileSpan::new(parsed.file, parsed.cst.node(entity.cst_id).span()),
                        definition,
                    ));
                }
            }
        }
        for reexport in &parsed.directives.reexports {
            let Some(source) = self.apis.get(&reexport.module) else {
                continue;
            };
            let source = source.visible_to(package);
            let ModuleSelection::Only(entities) = &reexport.selection else {
                continue;
            };
            for entity in entities {
                if let Some(definition) = source.definition(entity.name.0) {
                    resolutions.push((FileSpan::new(parsed.file, entity.name.1), definition));
                    resolutions.push((
                        FileSpan::new(parsed.file, parsed.cst.node(entity.cst_id).span()),
                        definition,
                    ));
                    if let Some(rename) = entity.rename {
                        resolutions.push((FileSpan::new(parsed.file, rename.1), definition));
                    }
                }
            }
        }
        resolutions
    }

    fn collect_imported_kind_ids(
        &self,
        imports: &[EffectiveImport],
        package: PackageId,
    ) -> FxHashMap<TyConId, solveig_ast::ast::Kind> {
        let mut kinds = FxHashMap::default();

        for import in imports {
            if let Some(api) = self.typed_apis.get(&import.module) {
                let module_api = self.apis.get(&import.module).unwrap_or_else(|| {
                    solveig_base::ice!("typed imported API missing its module API")
                });
                let filtered = api.imported_view(module_api, package, &import.spec);
                for (definition, kind) in &filtered.type_kinds {
                    let id = filtered
                        .type_ids
                        .get(definition)
                        .copied()
                        .expect("ICE: imported type kind missing typed API TyConId");
                    kinds.insert(id, kind.clone());
                }
            }
        }

        kinds
    }

    fn collect_imported_type_aliases(
        &self,
        imports: &[EffectiveImport],
        package: PackageId,
    ) -> FxHashMap<DefId, TypeAlias> {
        let mut aliases = FxHashMap::default();

        for import in imports {
            if let Some(api) = self.typed_apis.get(&import.module) {
                let module_api = self.apis.get(&import.module).unwrap_or_else(|| {
                    solveig_base::ice!("typed imported API missing its module API")
                });
                aliases.extend(api.imported_view(module_api, package, &import.spec).type_aliases);
            }
        }

        aliases
    }
}
