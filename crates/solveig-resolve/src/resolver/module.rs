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
                imported.tycon_kind_ids.len(),
                imported.class_param_kind_ids.len(),
                local.classes
            ),
        );

        let raw_data_defs = self.collect_raw_data_defs(module_name, parsed, cx, &local);
        if let Some(errors) = self.take_errors() {
            return Err(ResolverError::Resolve(errors));
        }

        let (mut data_defs, mut local_tycon_kind_ids, mut constructor_types) =
            self.resolve_data_defs(module_name, &raw_data_defs, imported)?;
        self.seed_compiler_prelude_defs(
            module_name,
            &mut data_defs,
            &mut local_tycon_kind_ids,
            &mut constructor_types,
        );

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

        let mut func_schemes = self.resolve_signatures(
            module_name,
            parsed,
            cx,
            imported,
            &local,
            &local_tycon_kind_ids,
            &class_defs,
        )?;
        let mut builtin_defs = Vec::new();
        self.seed_compiler_builtin_terms(cx, &mut func_schemes, &mut builtin_defs);
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
            builtin_defs,
            tycon_table: Default::default(),
            ctor_table: Default::default(),
            class_table: Default::default(),
            tycon_kinds: local_tycon_kind_ids,
            constructor_types,
            func_schemes,
            func_defs,
        };
        module.rebuild_identity_tables(self.names);

        Ok(ResolvedModuleInfo {
            module,
            resolutions: self.resolutions,
        })
    }

    fn seed_compiler_prelude_defs(
        &self,
        module_name: &ModuleName,
        data_defs: &mut Vec<DataDefinition>,
        local_tycon_kind_ids: &mut FxHashMap<TyConId, Kind>,
        constructor_types: &mut FxHashMap<CtorId, Ty>,
    ) {
        if module_name.as_string(self.session) != COMPILER_PRELUDE_BUILTINS_MODULE {
            return;
        }

        data_defs.insert(0, compiler_bool_data_definition());
        local_tycon_kind_ids.extend(compiler_primitive_tycon_kinds());
        local_tycon_kind_ids.insert(PRIM_TYS.bool, Kind::Star);
        constructor_types.extend(compiler_bool_constructor_types());
    }

    fn seed_compiler_builtin_terms(
        &self,
        cx: &ModuleContext,
        func_schemes: &mut FxHashMap<DefId, Scheme>,
        builtin_defs: &mut Vec<BuiltinDefinition>,
    ) {
        for builtin in &cx.builtins.builtin_defs {
            if builtin.id.module != cx.module_id {
                continue;
            }

            func_schemes.insert(builtin.id, builtin.scheme.clone());
            builtin_defs.push(builtin.clone());
        }
    }
}
