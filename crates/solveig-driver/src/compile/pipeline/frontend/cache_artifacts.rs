impl<'a> FrontendState<'a> {
    fn capture_incremental_cache(&self) -> IncrementalCache {
        let mut module_ids = FxHashMap::default();
        let mut source_hashes = FxHashMap::default();

        for (module_name, module_file) in &self.project.modules {
            if is_compiler_prelude_module(self.session, module_name) {
                continue;
            }

            let module_id = self
                .project
                .module_table
                .id(module_name)
                .expect("ICE: cache capture referenced unknown module");

            if !self.checked.contains_key(&module_id) {
                continue;
            }

            module_ids.insert(module_name.clone(), module_id);
            source_hashes.insert(module_name.clone(), module_file.source_hash);
        }

        IncrementalCache {
            module_ids,
            source_hashes,
            names: self.names.clone(),
            apis: self.apis.clone(),
            typed_apis: self.typed_apis.clone(),
            checked: self.checked.clone(),
            module_files: self.module_files.clone(),
            resolutions: self.resolutions.clone(),
            non_reusable_modules: self.non_reusable_modules.clone(),
            next_instance_id: self.next_instance_id,
            verified_core: self.verify_core,
        }
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
            !self.checked.contains_key(&module_id)
                || !self.typed_apis.contains_key(module_name)
                || !self.resolutions.contains_key(&module_id)
        }) {
            return;
        }

        let mut project_modules = FxHashMap::default();
        let mut module_ids = FxHashMap::default();
        let mut apis = FxHashMap::default();
        let mut typed_apis = FxHashMap::default();
        let mut checked = FxHashMap::default();
        let mut module_files = FxHashMap::default();
        let mut resolutions = FxHashMap::default();

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
                        solveig_base::ice!(
                            "prelude cache missing project module {}",
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
                        solveig_base::ice!(
                            "prelude cache missing module API {}",
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
                        solveig_base::ice!(
                            "prelude cache missing typed API {}",
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
                        solveig_base::ice!(
                            "prelude cache missing checked module {}",
                            module_name.as_string(self.session)
                        )
                    })
                    .clone(),
            );
            module_files.insert(
                module_id,
                *self.module_files.get(&module_id).unwrap_or_else(|| {
                    solveig_base::ice!(
                        "prelude cache missing file for {}",
                        module_name.as_string(self.session)
                    )
                }),
            );
            resolutions.insert(
                module_id,
                self.resolutions
                    .get(&module_id)
                    .unwrap_or_else(|| {
                        solveig_base::ice!(
                            "prelude cache missing resolutions for {}",
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
            checked,
            module_files,
            resolutions,
            next_instance_id: self.next_instance_id,
            verified_core: self.verify_core,
        });
    }
}

fn rebase_resolutions_file(
    resolutions: &[(FileSpan, DefId)],
    file: FileId,
) -> Vec<(FileSpan, DefId)> {
    resolutions
        .iter()
        .map(|(span, def_id)| (FileSpan::new(file, span.span), *def_id))
        .collect()
}
