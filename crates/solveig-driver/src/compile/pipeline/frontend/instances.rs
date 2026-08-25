// Graph-wide instance collection and resolver-diagnostic retention.

impl FrontendState<'_> {
    fn extend_instance_support(&self, env: &mut TypeCheckEnv) {
        for module in self.resolved.values() {
            env.class_defs.extend(
                module
                    .class_defs
                    .iter()
                    .map(|class_def| (class_def.id, class_def.clone())),
            );
            env.tycon_kinds.extend(module.tycon_kinds.clone());
            env.constructor_types
                .extend(module.constructor_types.clone());
            for (id, info) in module.tycon_table.iter() {
                env.tycon_table.insert_with_id(id, info.clone());
            }
            for (id, info) in module.ctor_table.iter() {
                env.ctor_table.insert_with_id(id, info.clone());
            }
            for (id, info) in module.class_table.iter() {
                env.class_table.insert_with_id(id, info.clone());
            }
        }
    }

    fn rebuild_instance_defs(&mut self, order: &[ModuleName]) {
        self.instance_defs.clear();
        for module_name in order {
            let Some(module_id) = self.project.module_table.id(module_name) else {
                continue;
            };
            let Some(module) = self.resolved.get(&module_id) else {
                continue;
            };
            self.instance_defs
                .extend(module.instance_defs.iter().cloned().map(|mut instance| {
                    instance.methods.clear();
                    instance
                }));
        }
    }

    fn validate_instance_graph(&mut self, order: &[ModuleName]) -> Option<FrontendPhase> {
        let resolved_modules = order
            .iter()
            .map(|module_name| {
                let module_id = self.project.module_table.id(module_name).unwrap_or_else(|| {
                    solveig_base::ice!("instance graph referenced an unknown module")
                });
                let module = self.resolved.get(&module_id).unwrap_or_else(|| {
                    solveig_base::ice!("instance coherence ran before every module resolved")
                });
                let file = self.parsed.get(self.session, module_name).file;
                (file, module)
            })
            .collect::<Vec<_>>();
        let errors = validate_instance_coherence(
            self.names,
            &self.project.module_table,
            &resolved_modules,
        );
        if errors.is_empty() {
            return None;
        }

        for error in errors {
            let file = match &error {
                ResolveError::OrphanInstance { span, .. }
                | ResolveError::OverlappingInstances { span, .. } => span.file,
                _ => solveig_base::ice!("instance coherence returned a non-coherence error"),
            };
            let module = self
                .project
                .modules
                .iter()
                .find(|(_, module)| module.parsed.file == file)
                .map_or_else(
                    || {
                        solveig_base::ice!(
                            "instance coherence error referenced an unknown file"
                        )
                    },
                    |(name, _)| name.clone(),
                );
            self.resolve_errors.push(ModuleResolveErrors {
                module,
                errors: vec![error.clone()],
            });
            push_resolve_errors(
                self.session,
                Some(&self.project.module_table),
                vec![error],
            );
        }

        Some(FrontendPhase::Resolve)
    }
}
