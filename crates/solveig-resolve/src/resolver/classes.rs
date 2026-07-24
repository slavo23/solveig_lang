impl<'a> Resolver<'a> {
    fn resolve_class_ref(
        &mut self,
        r: &NameRef,
        span: FileSpan,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> String {
        match self.resolve_class_defid(r, span, cx, local) {
            Some(def_id) => {
                self.record_resolution(span, def_id);
                self.qualified_def_name(def_id)
            }
            None => self.unresolved_name(r),
        }
    }
    fn resolve_class_defid(
        &mut self,
        r: &NameRef,
        span: FileSpan,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Option<DefId> {
        match r.segments.as_slice() {
            [name] => self.resolve_unqualified_class_defid(name, span, cx, local),

            [_first, rest @ ..] => {
                let local_name = *rest
                    .last()
                    .expect("ICE: qualified class name must have a last segment");

                if cx.module.0.as_slice() == r.prefix() {
                    if let Some(sym) = cx.local_api.classes.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(span, ResolveNameKind::Class, self.unresolved_name(r));
                    return None;
                }

                if let Some(imp) = self.imported_by_alias_or_module(r, cx) {
                    if let Some(sym) = imp.api.classes.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(span, ResolveNameKind::Class, self.unresolved_name(r));
                    return None;
                }

                self.report_unknown_module(span, ModuleName(r.prefix().to_vec()));
                None
            }

            [] => panic!("ICE: empty class name ref"),
        }
    }
    fn resolve_unqualified_class_defid(
        &mut self,
        name: &Symbol,
        span: FileSpan,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Option<DefId> {
        if let Some(def_id) = local.classes.get(name) {
            return Some(*def_id);
        }

        if let Some(sym) = cx.local_api.classes.get(name) {
            return Some(*sym);
        }

        let mut candidates = Vec::new();

        for imp in &cx.imported_apis {
            if let Some(sym) = imp.api.classes.get(name)
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
                    ResolveNameKind::Class,
                    self.symbol_text(*name).to_string(),
                );
                None
            }
            _ => {
                self.report_ambiguous_import(span, ResolveNameKind::Class, *name, candidates);
                None
            }
        }
    }
    fn canonicalize_predicate(
        &self,
        pred: Predicate,
        _cx: &ModuleContext,
        _local: &LocalResolverEnv,
    ) -> Predicate {
        pred
    }
    fn canonicalize_scheme_classes(
        &self,
        scheme: Scheme,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Scheme {
        Scheme {
            quantifiers: scheme.quantifiers,
            predicates: scheme
                .predicates
                .into_iter()
                .map(|pred| self.canonicalize_predicate(pred, cx, local))
                .collect(),
            ty: scheme.ty,
        }
    }
    fn class_param_kind_env(
        imported: &ResolverImports,
        class_defs: &[ClassDefinition],
    ) -> HashMap<ClassId, Vec<Kind>> {
        let mut class_env = imported.class_param_kind_ids.clone();

        class_env.extend(class_defs.iter().map(|class_def| {
            (
                class_def.id,
                class_def
                    .params
                    .iter()
                    .map(|(_, kind)| kind.clone())
                    .collect::<Vec<_>>(),
            )
        }));

        class_env
    }
    fn kind_check_instance_predicates(
        &self,
        context: Vec<Predicate>,
        head: Predicate,
        all_tycon_kinds: &HashMap<TyConId, Kind>,
        class_env: &HashMap<ClassId, Vec<Kind>>,
    ) -> Result<(Vec<Predicate>, Predicate), KindError> {
        let context_len = context.len();

        let mut predicates = context;
        predicates.push(head);

        // Reuse signature kind inference as a predicate-kind checker.
        //
        // It already knows how to:
        // - create implicit kinded quantifiers for free type variables in predicates;
        // - look up class parameter kinds;
        // - reject ill-kinded heads like `Functor Int` because `Int : *`, but
        //   `Functor` expects an argument of kind `* -> *`.
        let pseudo_scheme = Scheme {
            quantifiers: Vec::new(),
            predicates,
            ty: TypeExpr::TyUnit,
        };

        let info = SignatureKindInferencer::with_envs(all_tycon_kinds.clone(), class_env.clone())
            .infer_scheme(&pseudo_scheme)?;

        let mut predicates = info.scheme.predicates;
        let checked_head = predicates
            .pop()
            .expect("ICE: instance kind checking lost instance head predicate");

        debug_assert_eq!(
            predicates.len(),
            context_len,
            "instance kind checking changed predicate count unexpectedly"
        );

        Ok((predicates, checked_head))
    }
    fn resolve_surface_predicate(
        &mut self,
        pred: &SpannedWrapper<SurfacePredicate>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Predicate {
        if pred.0.args.len() != 1 {
            self.errors.push(ResolveError::ClassArity {
                span: self.file_span(cx, pred.1),
                class_name: pred.0.class_name.0.display(self.session),
                actual: pred.0.args.len(),
            });
        }

        let resolved_class_name = self.resolve_class_ref(
            &pred.0.class_name.0,
            self.file_span(cx, pred.0.class_name.1),
            cx,
            local,
        );

        let resolved = Predicate::legacy(
            resolved_class_name,
            pred
                .0
                .args
                .iter()
                .map(|arg| self.resolve_surface_type_in_module(arg, cx, local))
                .collect(),
        );

        self.canonicalize_predicate(resolved, cx, local)
    }
    fn resolve_class_defs(
        &mut self,
        module_name: &ModuleName,
        parsed: &ParsedModule,
        cx: &ModuleContext,
        imported: &ResolverImports,
        local: &LocalResolverEnv,
        all_tycon_kinds: &HashMap<TyConId, Kind>,
    ) -> Result<Vec<ClassDefinition>, KindError> {
        let raw_class_defs = parsed
            .defs
            .iter()
            .filter_map(|defn| match &defn.0 {
                SurfaceDefinition::ClassDef {
                    name,
                    ty_params,
                    superclasses,
                    methods,
                    defaults,
                } => {
                    if ty_params.len() != 1 {
                        self.errors.push(ResolveError::ClassArity {
                            span: self.file_span(cx, name.1),
                            class_name: self.symbol_text(name.0).to_string(),
                            actual: ty_params.len(),
                        });
                    }

                    let class_def_id = self.local_def_id(cx, name.0);
                    let class_name = self.qualified_def_name(class_def_id);
                    let class_id = legacy_class_id(&class_name);

                    let class_params = ty_params
                        .iter()
                        .map(|param| (self.symbol_text(param.0).to_string(), Kind::Star))
                        .collect::<Vec<_>>();

                    let class_head_pred = Predicate::legacy(
                        class_name.clone(),
                        ty_params
                            .iter()
                            .map(|param| TypeExpr::make_var(self.symbol_text(param.0).to_string()))
                            .collect(),
                    );

                    let resolved_superclasses = superclasses
                        .iter()
                        .map(|pred| self.resolve_surface_predicate(pred, cx, local))
                        .collect::<Vec<_>>();

                    trace_predicates(
                        self.session,
                        module_name,
                        &format!("class {} superclasses", class_name),
                        &resolved_superclasses,
                    );

                    let resolved_methods = methods
                        .iter()
                        .map(|method| {
                            let method_id = self.local_def_id(cx, method.name.0);
                            let raw_method_scheme =
                                self.resolve_surface_scheme(&method.ty, cx, local);

                            let mut predicates =
                                Vec::with_capacity(1 + raw_method_scheme.predicates.len());
                            predicates.push(class_head_pred.clone());
                            predicates.extend(raw_method_scheme.predicates.clone());

                            let scheme = Scheme {
                                quantifiers: raw_method_scheme.quantifiers.clone(),
                                predicates,
                                ty: raw_method_scheme.ty,
                            };

                            trace_predicates(
                                self.session,
                                module_name,
                                &format!(
                                    "class method {}.{} predicates",
                                    class_name,
                                    self.symbol_text(method.name.0)
                                ),
                                &scheme.predicates,
                            );

                            ClassMethodDef {
                                id: method_id,
                                name: self.qualified_def_name(method_id),
                                short_name: self.symbol_text(method.name.0).to_string(),
                                scheme,
                                default_body: defaults
                                    .iter()
                                    .find(|default| default.name.0 == method.name.0)
                                    .map(|default| {
                                        self.wrap_function_body_without_signature_in_module(
                                            &default.body,
                                            &default.params,
                                            cx,
                                            local,
                                        )
                                    }),
                            }
                        })
                        .collect::<Vec<_>>();

                    Some((
                        ClassDefinition {
                            id: class_id,
                            name: class_name,
                            short_name: self.symbol_text(name.0).to_string(),
                            params: class_params,
                            superclasses: resolved_superclasses,
                            methods: resolved_methods,
                        },
                        self.file_span(cx, defn.1),
                    ))
                }

                _ => None,
            })
            .collect::<Vec<_>>();

        if !self.errors.is_empty() {
            return Ok(Vec::new());
        }

        let mut seed_class_env = imported.class_param_kind_ids.clone();
        let mut next_seed_kind_meta = u32::MAX;

        seed_class_env.extend(raw_class_defs.iter().map(|(class_def, _span)| {
            (
                class_def.id,
                class_def
                    .params
                    .iter()
                    .map(|(_, _)| {
                        let id = KindMetaId(next_seed_kind_meta);
                        next_seed_kind_meta = next_seed_kind_meta
                            .checked_sub(1)
                            .expect("ICE: exhausted class seed kind metas");
                        Kind::make_meta(id)
                    })
                    .collect::<Vec<_>>(),
            )
        }));

        let kinded_class_defs = raw_class_defs
            .iter()
            .map(|(class_def, span)| {
                ClassKindInferencer::new(all_tycon_kinds.clone(), seed_class_env.clone())
                    .infer_class(class_def)
                    .map_err(|error| error.with_span(*span))
            })
            .collect::<Result<Vec<_>, _>>()?;

        trace(
            self.session,
            module_name,
            format!(
                "classes: {:?}",
                kinded_class_defs
                    .iter()
                    .map(|class_def| (&class_def.name, &class_def.params))
                    .collect::<Vec<_>>()
            ),
        );

        Ok(kinded_class_defs)
    }
    fn unreported_tycon_defid(&self, r: &NameRef, cx: &ModuleContext) -> Option<DefId> {
        match r.segments.as_slice() {
            [name] => cx
                .builtins
                .types
                .get(name)
                .or_else(|| cx.local_api.types.get(name))
                .copied()
                .or_else(|| {
                    let mut candidates = cx
                        .imported_apis
                        .iter()
                        .filter_map(|imp| imp.api.types.get(name).copied())
                        .collect::<Vec<_>>();
                    candidates.sort_unstable();
                    candidates.dedup();

                    match candidates.as_slice() {
                        [id] => Some(*id),
                        _ => None,
                    }
                }),

            [_first, rest @ ..] => {
                let local_name = *rest
                    .last()
                    .expect("ICE: qualified type constructor name must have a last segment");

                if cx.module.0.as_slice() == r.prefix() {
                    return cx.local_api.types.get(&local_name).copied();
                }

                self.imported_by_alias_or_module(r, cx)
                    .and_then(|imp| imp.api.types.get(&local_name).copied())
            }

            [] => None,
        }
    }

    fn instance_type_head_module(
        &self,
        ty: &SpannedWrapper<SurfaceTy>,
        cx: &ModuleContext,
    ) -> Option<ModuleId> {
        match &ty.0 {
            SurfaceTy::Con(con) => self.unreported_tycon_defid(&con.0, cx).map(|id| id.module),
            SurfaceTy::App(lhs, _) => self.instance_type_head_module(lhs, cx),
            SurfaceTy::Unit
            | SurfaceTy::Var(_)
            | SurfaceTy::Arr(_, _)
            | SurfaceTy::ForAll(_, _)
            | SurfaceTy::Qualified(_, _) => None,
        }
    }

    fn resolve_instance_defs(
        &mut self,
        module_name: &ModuleName,
        parsed: &ParsedModule,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
        all_tycon_kinds: &HashMap<TyConId, Kind>,
        class_env: &HashMap<ClassId, Vec<Kind>>,
    ) -> Result<Vec<InstanceDefinition<Span>>, KindError> {
        let mut instance_defs = Vec::new();

        for defn in parsed.defs.iter() {
            let SurfaceDefinition::InstanceDef {
                context,
                class_name,
                args,
                methods,
            } = &defn.0
            else {
                continue;
            };

            let class_ref = NameRef {
                segments: vec![class_name.0],
            };
            let class_span = self.file_span(cx, class_name.1);
            let class_def_id = self.resolve_class_defid(&class_ref, class_span, cx, local);
            let resolved_class_name = match class_def_id {
                Some(def_id) => {
                    self.record_resolution(class_span, def_id);
                    self.qualified_def_name(def_id)
                }
                None => self.unresolved_name(&class_ref),
            };
            let type_module = args
                .first()
                .and_then(|arg| self.instance_type_head_module(arg, cx));

            let raw_head = Predicate::legacy(
                resolved_class_name,
                args.iter()
                    .map(|arg| self.resolve_surface_type_in_module(arg, cx, local))
                    .collect(),
            );

            let raw_head = self.canonicalize_predicate(raw_head, cx, local);

            if args.len() != 1 {
                self.errors.push(ResolveError::ClassArity {
                    span: self.file_span(cx, class_name.1),
                    class_name: self.symbol_text(class_name.0).to_string(),
                    actual: args.len(),
                });
            }

            let raw_context = context
                .iter()
                .map(|pred| self.resolve_surface_predicate(pred, cx, local))
                .collect::<Vec<_>>();

            trace_predicates(
                self.session,
                module_name,
                "instance raw context",
                &raw_context,
            );
            trace_predicates(
                self.session,
                module_name,
                "instance raw head",
                std::slice::from_ref(&raw_head),
            );

            if !self.errors.is_empty() {
                continue;
            }

            let (checked_context, checked_head) = self
                .kind_check_instance_predicates(raw_context, raw_head, all_tycon_kinds, class_env)
                .map_err(|error| error.with_span(self.file_span(cx, defn.1)))?;

            trace_predicates(
                self.session,
                module_name,
                "instance checked context",
                &checked_context,
            );
            trace_predicates(
                self.session,
                module_name,
                "instance checked head",
                std::slice::from_ref(&checked_head),
            );

            let resolved_methods = methods
                .iter()
                .map(|method| InstanceMethodDef {
                    name: self.symbol_text(method.name.0).to_string(),
                    body: self.wrap_function_body_without_signature_in_module(
                        &method.body,
                        &method.params,
                        cx,
                        local,
                    ),
                })
                .collect::<Vec<_>>();

            instance_defs.push(InstanceDefinition {
                id: solveig_ast::ast::InstanceId(
                    u32::try_from(instance_defs.len())
                        .expect("ICE: module instance count does not fit in u32"),
                ),
                span: defn.1.to_usize_range(),
                def_module: cx.module_id,
                class_module: class_def_id.map(|id| id.module),
                type_module,
                context: checked_context,
                context_evidence: Vec::new(),
                head: checked_head,
                superclass_evidence: Vec::new(),
                methods: resolved_methods,
            });
        }

        Ok(instance_defs)
    }
}
