// Import and module-directive projections used by frontend orchestration.

impl FrontendState<'_> {
    fn build_imported_apis(
        &self,
        imports: &[EffectiveImport],
        package: PackageId,
    ) -> Result<Vec<ImportedApi>, Vec<ResolveError>> {
        let mut projected = Vec::with_capacity(imports.len());
        let mut qualifiers = FxHashMap::<Symbol, (ModuleName, FileSpan)>::default();
        let mut errors = Vec::new();
        for import in imports {
            if import.qualifies {
                if let Some((previous, _)) = qualifiers.get(&import.qualifier)
                    && previous != &import.module
                {
                    errors.push(ResolveError::AmbiguousModuleQualifier {
                        span: import.span,
                        qualifier: import.qualifier,
                        modules: vec![previous.clone(), import.module.clone()],
                    });
                }
                qualifiers
                    .entry(import.qualifier)
                    .or_insert_with(|| (import.module.clone(), import.span));
            }
            let api = self
                .apis
                .get(&import.module)
                .unwrap_or_else(|| {
                    solveig_base::ice!(
                        "imported module API missing for {}",
                        import.module.as_string(self.session)
                    )
                })
                .visible_to(package);
            let unqualified = match project_module_selection_diagnostics(
                &api,
                import.span.file,
                &import.module,
                &import.selection,
            ) {
                Ok(projected) => projected,
                Err(mut import_errors) => {
                    errors.append(&mut import_errors);
                    continue;
                }
            };
            projected.push(ImportedApi {
                qualifier: import.qualifier,
                qualifies: import.qualifies,
                accepts_module_name: import.accepts_module_name,
                module: import.module.clone(),
                api: Arc::new(api),
                unqualified,
            });
        }
        if errors.is_empty() {
            Ok(projected)
        } else {
            Err(errors)
        }
    }

    fn build_imported_typed_apis(
        &self,
        imports: &[EffectiveImport],
        imported_apis: &[ImportedApi],
        package: PackageId,
    ) -> Vec<ImportedTypedApi> {
        imports
            .iter()
            .zip(imported_apis)
            .filter_map(|(import, imported_api)| {
                self.typed_apis.get(&import.module).map(|api| {
                    let module_api = self.apis.get(&import.module).unwrap_or_else(|| {
                        solveig_base::ice!("typed imported API missing its module API")
                    });
                    let api = api.visible_to(module_api, package);
                    let unqualified = api.selected_by_api(&imported_api.unqualified);
                    ImportedTypedApi {
                        qualifier: import.qualifier,
                        qualifies: import.qualifies,
                        accepts_module_name: import.accepts_module_name,
                        module: import.module.clone(),
                        api: Arc::new(api),
                        unqualified,
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
                let visible = api.visible_to(module_api, package);
                for (definition, kind) in &visible.type_kinds {
                    let id = visible
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
                aliases.extend(api.visible_to(module_api, package).type_aliases);
            }
        }

        aliases
    }
}
