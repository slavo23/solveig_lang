impl<'a> Resolver<'a> {
    pub fn resolve_module(
        mut self,
        module_name: &ModuleName,
        parsed: &ParsedModule,
        cx: &ModuleContext,
        imported: &ResolverImports,
    ) -> Result<ResolvedModuleInfo<Span>, ResolverError> {
        let local = LocalResolverEnv::from_parsed_module(cx.module_id, parsed);

        trace(
            self.session,
            module_name,
            format!(
                "start: defs={}, imported_tycons={}, imported_classes={}, local_classes={:?}",
                parsed.defs.len(),
                imported.tycon_kinds.len(),
                imported.class_param_kind_ids.len(),
                local.classes
            ),
        );

        let raw_data_defs = self.collect_raw_data_defs(module_name, parsed, cx, &local);
        if let Some(errors) = self.take_errors() {
            return Err(ResolverError::Resolve(errors));
        }

        let (data_defs, local_tycon_kind_ids, constructor_types) =
            self.resolve_data_defs(module_name, raw_data_defs, imported)?;

        let mut all_tycon_kind_ids = imported.tycon_kind_ids.clone();
        all_tycon_kind_ids.extend(local_tycon_kind_ids.clone());

        let class_defs =
            self.resolve_class_defs(module_name, parsed, cx, imported, &local, &all_tycon_kind_ids)?;
        if let Some(errors) = self.take_errors() {
            return Err(ResolverError::Resolve(errors));
        }

        let class_env = Self::class_param_kind_env(imported, &class_defs);

        let instance_defs = self.resolve_instance_defs(
            module_name,
            parsed,
            cx,
            &local,
            &all_tycon_kind_ids,
            &class_env,
        )?;
        if let Some(errors) = self.take_errors() {
            return Err(ResolverError::Resolve(errors));
        }

        let func_schemes = self.resolve_signatures(
            module_name,
            parsed,
            cx,
            imported,
            &local,
            &local_tycon_kind_ids,
            &class_defs,
        )?;
        if let Some(errors) = self.take_errors() {
            return Err(ResolverError::Resolve(errors));
        }

        let func_defs = self.resolve_function_defs(module_name, parsed, cx, &local, &func_schemes);
        if let Some(errors) = self.take_errors() {
            return Err(ResolverError::Resolve(errors));
        }

        trace(
            self.session,
            module_name,
            format!(
                "done: data={}, classes={}, instances={}, signatures={}, functions={}",
                data_defs.len(),
                class_defs.len(),
                instance_defs.len(),
                func_schemes.len(),
                func_defs.len()
            ),
        );

        let mut module = ResolvedModule {
            data_defs,
            class_defs,
            instance_defs,
            tycon_table: Default::default(),
            ctor_table: Default::default(),
            class_table: Default::default(),
            tycon_kinds: local_tycon_kind_ids,
            constructor_types,
            func_schemes,
            func_defs,
        };
        module.rebuild_identity_tables();

        Ok(ResolvedModuleInfo {
            module,
            resolutions: self.resolutions,
        })
    }
}
