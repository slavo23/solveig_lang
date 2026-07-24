impl<'a> Resolver<'a> {
    fn resolve_ctor_id(&mut self, r: &NameRef, span: FileSpan, cx: &ModuleContext) -> CtorId {
        match self.resolve_ctor_defid(r, span, cx) {
            Some(def_id) => {
                self.record_resolution(span, def_id);
                let qualified_name = self.qualified_def_name(def_id);

                cx.builtins
                    .constructor_ids
                    .get(&def_id)
                    .copied()
                    .unwrap_or_else(|| legacy_ctor_id(&qualified_name))
            }
            None => legacy_ctor_id(&self.unresolved_name(r)),
        }
    }

    fn resolve_ctor_defid(
        &mut self,
        r: &NameRef,
        span: FileSpan,
        cx: &ModuleContext,
    ) -> Option<DefId> {
        match r.segments.as_slice() {
            [name] => {
                if let Some(sym) = cx.builtins.constructors.get(name) {
                    return Some(*sym);
                }

                if let Some(sym) = cx.local_api.constructors.get(name) {
                    return Some(*sym);
                }

                let mut candidates = Vec::new();

                for imp in &cx.imported_apis {
                    if let Some(sym) = imp.api.constructors.get(name)
                        && !candidates.contains(sym)
                    {
                        candidates.push(*sym);
                    }
                }

                match candidates.as_slice() {
                    [sym] => Some(*sym),
                    [] => {
                        self.report_unknown_name(
                            span,
                            ResolveNameKind::Constructor,
                            self.symbol_text(*name).to_string(),
                        );
                        None
                    }
                    _ => {
                        self.report_ambiguous_import(
                            span,
                            ResolveNameKind::Constructor,
                            *name,
                            candidates,
                        );
                        None
                    }
                }
            }

            [_first, rest @ ..] => {
                let local_name = *rest
                    .last()
                    .expect("ICE: qualified constructor name must have a last segment");

                if cx.module.0.as_slice() == r.prefix() {
                    if let Some(sym) = cx.local_api.constructors.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(
                        span,
                        ResolveNameKind::Constructor,
                        self.unresolved_name(r),
                    );
                    return None;
                }

                if let Some(imp) = self.imported_by_alias_or_module(r, cx) {
                    if let Some(sym) = imp.api.constructors.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(
                        span,
                        ResolveNameKind::Constructor,
                        self.unresolved_name(r),
                    );
                    return None;
                }

                self.report_unknown_module(span, ModuleName(r.prefix().to_vec()));
                None
            }

            [] => panic!("ICE: empty constructor name ref"),
        }
    }
    fn resolve_surface_pattern_in_module(
        &mut self,
        sf: &SpannedWrapper<SurfacePattern>,
        cx: &ModuleContext,
    ) -> PatternNode<Span> {
        let span = to_core_span(sf.1);

        let node = match &sf.0 {
            SurfacePattern::Var(var_name) => Pattern::Var(self.symbol_text(*var_name).to_string()),
            SurfacePattern::Lit(lit) => {
                Pattern::Lit(resolve_literal(lit))
            }
            SurfacePattern::Wild => Pattern::Wild,
            SurfacePattern::Con(con_name, args) => Pattern::Con(
                self.resolve_ctor_id(
                    &con_name.0,
                    self.file_span(cx, con_name.1),
                    cx,
                ),
                args.iter()
                    .map(|arg| self.resolve_surface_pattern_in_module(arg, cx))
                    .collect(),
            ),
        };

        PatternNode::new(node, span)
    }
    fn collect_surface_pattern_binders(
        pat: &SpannedWrapper<SurfacePattern>,
        out: &mut HashSet<Symbol>,
    ) {
        match &pat.0 {
            SurfacePattern::Var(name) => {
                out.insert(*name);
            }
            SurfacePattern::Lit(_) => {}
            SurfacePattern::Wild => {}
            SurfacePattern::Con(_, args) => {
                for arg in args {
                    Self::collect_surface_pattern_binders(arg, out);
                }
            }
        }
    }
    fn resolve_surface_case_arm_in_module(
        &mut self,
        sf: &SpannedWrapper<SurfaceMatchArm>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
        bound_terms: &HashSet<Symbol>,
    ) -> CaseArmNode<Span> {
        let pattern = self.resolve_surface_pattern_in_module(&sf.0.pattern, cx);

        let mut arm_bound_terms = bound_terms.clone();
        Self::collect_surface_pattern_binders(&sf.0.pattern, &mut arm_bound_terms);

        CaseArmNode::new(
            CaseArm {
                pattern,
                expr: self.resolve_surface_expr_in_module(&sf.0.expr, cx, local, &arm_bound_terms),
            },
            to_core_span(sf.1),
        )
    }
    fn collect_raw_data_defs(
        &mut self,
        _module_name: &ModuleName,
        parsed: &ParsedModule,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Vec<RawDataDef> {
        parsed
            .defs
            .iter()
            .filter_map(|defn| match &defn.0 {
                SurfaceDefinition::DataDef {
                    name: spanned_name,
                    ty_params,
                    constructors,
                } => {
                    let (name, _span) = spanned_name;
                    let data_name = self.qualified_def_name(self.local_def_id(cx, *name));

                    Some(RawDataDef {
                        id: legacy_tycon_id(&data_name),
                        name: data_name,
                        short_name: self.symbol_text(*name).to_string(),
                        span: self.file_span(cx, defn.1),
                        ty_params: ty_params
                            .iter()
                            .map(|p| self.symbol_text(p.0).to_string())
                            .collect(),
                        constructors: constructors
                            .iter()
                            .map(|constructor| RawConstructorDef {
                                name: self.qualified_def_name(
                                    self.local_def_id(cx, constructor.0.name.0),
                                ),
                                short_name: self.symbol_text(constructor.0.name.0).to_string(),
                                span: self.file_span(cx, constructor.1),
                                fields: constructor
                                    .0
                                    .params
                                    .iter()
                                    .map(|ty| self.resolve_surface_type_in_module(ty, cx, local))
                                    .collect(),
                            })
                            .collect(),
                    })
                }

                _ => None,
            })
            .collect()
    }
    fn resolve_data_defs(
        &self,
        module_name: &ModuleName,
        raw_data_defs: Vec<RawDataDef>,
        imported: &ResolverImports,
    ) -> Result<
        (
            Vec<DataDefinition>,
            HashMap<TyConId, Kind>,
            HashMap<CtorId, TypeExpr>,
        ),
        KindError,
    > {
        let mut data_tycon_kind_env = imported.tycon_kind_ids.clone();
        if module_name.as_string(self.session) == COMPILER_PRELUDE_BUILTINS_MODULE {
            data_tycon_kind_env.extend(compiler_primitive_tycon_kinds());
            data_tycon_kind_env.insert(PRIM_TYS.bool, Kind::Star);
        }

        let (inferred_data, _all_tycon_kinds) =
            infer_all_data_kinds_with_env(raw_data_defs, data_tycon_kind_env)?;

        let mut data_defs = Vec::new();
        let mut local_tycon_kind_ids = HashMap::new();
        let mut constructor_types = HashMap::new();

        for idef in &inferred_data {
            let mut core_ctors = Vec::new();

            for ctor in &idef.constructors {
                let ctor_id = legacy_ctor_id(&ctor.name);
                constructor_types.insert(ctor_id, ctor.full_type.clone());

                core_ctors.push(ConstructorDef {
                    id: ctor_id,
                    name: ctor.name.clone(),
                    short_name: ctor.short_name.clone(),
                    fields: ctor.kinded_fields.clone(),
                    full_type: ctor.full_type.clone(),
                });
            }

            local_tycon_kind_ids.insert(idef.id, idef.tycon_kind.clone());

            data_defs.push(DataDefinition {
                name: idef.name.clone(),
                short_name: idef.short_name.clone(),
                ty_params: idef.ty_params.clone(),
                tycon_kind: idef.tycon_kind.clone(),
                constructors: core_ctors,
            });
        }

        trace(
            self.session,
            module_name,
            format!(
                "data: local_tycons={:?}, local_ctors={:?}",
                local_tycon_kind_ids.keys().collect::<Vec<_>>(),
                constructor_types.keys().collect::<Vec<_>>()
            ),
        );

        Ok((data_defs, local_tycon_kind_ids, constructor_types))
    }
}
