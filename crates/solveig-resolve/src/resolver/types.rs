impl<'a> Resolver<'a> {
    fn resolve_type_aliases(&mut self, cx: &ModuleContext, local: &LocalResolverEnv) {
        let mut aliases = local.type_aliases.keys().copied().collect::<Vec<_>>();
        aliases.sort_unstable();

        for name in aliases {
            let def_id = self.local_def_id(cx, name);
            let _ = self.resolve_local_type_alias(def_id, cx, local);
        }
    }

    fn resolve_local_type_alias(
        &mut self,
        def_id: DefId,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Option<TypeAlias> {
        if let Some(alias) = self.type_aliases.get(&def_id) {
            return Some(alias.clone());
        }

        let (defn, span, _) = local.type_aliases.get(&def_id.name).cloned()?;
        let SurfaceDefinition::TypeAliasDef {
            name,
            ty_params,
            ty,
            ..
        } = defn
        else {
            solveig_base::ice!("local type alias cache contained a non-alias definition");
        };

        self.type_alias_stack.push(def_id);
        self.push_tyvar_scope();
        let params = ty_params
            .iter()
            .map(|param| self.bind_tyvar_in_current_scope(param.0))
            .collect::<Vec<_>>();
        let ty = self.resolve_surface_type_in_module(&ty, cx, local);
        self.pop_tyvar_scope();
        self.type_alias_stack.pop();

        let alias = TypeAlias {
            id: def_id,
            params,
            ty,
            span: self.file_span(cx, span),
        };
        self.record_resolution(self.file_span(cx, name.1), def_id);
        self.type_aliases.insert(def_id, alias.clone());
        Some(alias)
    }

    fn resolve_type_alias_by_defid(
        &mut self,
        def_id: DefId,
        span: FileSpan,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Option<TypeAlias> {
        if let Some(alias) = self.type_aliases.get(&def_id) {
            return Some(alias.clone());
        }

        if def_id.module != cx.module_id {
            return self.imported_type_aliases.get(&def_id).cloned();
        }

        if let Some(index) = self
            .type_alias_stack
            .iter()
            .position(|stacked| *stacked == def_id)
        {
            let Some(cycle) = self.type_alias_stack.get(index..) else {
                solveig_base::ice!("type alias cycle stack index out of bounds");
            };
            let mut names = cycle
                .iter()
                .map(|alias| alias.name)
                .collect::<Vec<_>>();
            names.push(def_id.name);
            self.errors
                .push(ResolveError::CyclicTypeAlias { span, names });
            return None;
        }

        self.resolve_local_type_alias(def_id, cx, local)
    }

    fn resolve_type_alias_defid(
        &mut self,
        r: &NameRef,
        span: FileSpan,
        cx: &ModuleContext,
    ) -> Result<Option<DefId>, ()> {
        match r.segments.as_slice() {
            [name] => {
                if cx.builtins.types.contains_key(name) || cx.local_api.types.contains_key(name) {
                    return Ok(None);
                }

                if let Some(def_id) = cx.local_api.type_aliases.get(name) {
                    return Ok(Some(*def_id));
                }

                let mut type_candidates = Vec::new();
                let mut alias_candidates = Vec::new();

                for imp in &cx.imported_apis {
                    if let Some(def_id) = imp.api.types.get(name)
                        && !type_candidates.contains(def_id)
                    {
                        type_candidates.push(*def_id);
                    }

                    if let Some(def_id) = imp.api.type_aliases.get(name)
                        && !alias_candidates.contains(def_id)
                    {
                        alias_candidates.push(*def_id);
                    }
                }

                if alias_candidates.is_empty() {
                    return Ok(None);
                }

                if type_candidates.is_empty() && alias_candidates.len() == 1 {
                    let Some(def_id) = alias_candidates.first().copied() else {
                        solveig_base::ice!("single alias candidate check lost the candidate");
                    };
                    return Ok(Some(def_id));
                }

                alias_candidates.extend(type_candidates);
                alias_candidates.sort_unstable();
                alias_candidates.dedup();
                self.report_ambiguous_import(
                    span,
                    ResolveNameKind::TypeConstructor,
                    *name,
                    alias_candidates,
                );
                Err(())
            }

            [_first, rest @ ..] => {
                let Some(local_name) = rest.last().copied() else {
                    solveig_base::ice!("qualified type alias name must have a last segment");
                };

                if cx.module.0.as_slice() == r.prefix() {
                    if cx.local_api.types.contains_key(&local_name) {
                        return Ok(None);
                    }

                    return Ok(cx.local_api.type_aliases.get(&local_name).copied());
                }

                let Some(imp) = self.imported_by_alias_or_module(r, cx) else {
                    return Ok(None);
                };

                if imp.api.types.contains_key(&local_name) {
                    return Ok(None);
                }

                Ok(imp.api.type_aliases.get(&local_name).copied())
            }

            [] => solveig_base::ice!("empty type alias name ref"),
        }
    }

    fn unreported_type_alias_defid(&self, r: &NameRef, cx: &ModuleContext) -> Option<DefId> {
        match r.segments.as_slice() {
            [name] => {
                if cx.builtins.types.contains_key(name) || cx.local_api.types.contains_key(name) {
                    return None;
                }

                if let Some(def_id) = cx.local_api.type_aliases.get(name) {
                    return Some(*def_id);
                }

                let mut candidates = cx
                    .imported_apis
                    .iter()
                    .filter_map(|imp| imp.api.type_aliases.get(name).copied())
                    .collect::<Vec<_>>();
                candidates.sort_unstable();
                candidates.dedup();

                match candidates.as_slice() {
                    [def_id] => Some(*def_id),
                    _ => None,
                }
            }

            [_first, rest @ ..] => {
                let local_name = *rest.last()?;

                if cx.module.0.as_slice() == r.prefix() {
                    if cx.local_api.types.contains_key(&local_name) {
                        return None;
                    }

                    return cx.local_api.type_aliases.get(&local_name).copied();
                }

                self.imported_by_alias_or_module(r, cx).and_then(|imp| {
                    if imp.api.types.contains_key(&local_name) {
                        None
                    } else {
                        imp.api.type_aliases.get(&local_name).copied()
                    }
                })
            }

            [] => None,
        }
    }

    fn type_application_head<'sf>(
        sf: &'sf SpannedWrapper<SurfaceTy>,
        args: &mut Vec<&'sf SpannedWrapper<SurfaceTy>>,
    ) -> Option<&'sf SpannedWrapper<NameRef>> {
        match &sf.0 {
            SurfaceTy::Con(con) => Some(con),
            SurfaceTy::App(lhs, rhs) => {
                args.push(rhs);
                Self::type_application_head(lhs, args)
            }
            _ => None,
        }
    }

    fn try_expand_type_alias_application(
        &mut self,
        sf: &SpannedWrapper<SurfaceTy>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Option<Ty> {
        let mut args = Vec::new();
        let head = Self::type_application_head(sf, &mut args)?;
        args.reverse();

        let head_span = self.file_span(cx, head.1);
        let def_id = match self.resolve_type_alias_defid(&head.0, head_span, cx) {
            Ok(Some(def_id)) => def_id,
            Ok(None) => return None,
            Err(()) => return Some(Ty::TyUnit),
        };
        self.record_resolution(head_span, def_id);

        let Some(alias) = self.resolve_type_alias_by_defid(def_id, head_span, cx, local) else {
            return Some(Ty::TyUnit);
        };

        let expected = alias.params.len();
        let actual = args.len();
        if actual < expected {
            self.errors.push(ResolveError::PartialTypeAlias {
                span: self.file_span(cx, sf.1),
                name: def_id.name,
                expected,
                actual,
            });
            return Some(Ty::TyUnit);
        }

        if actual > expected {
            self.errors.push(ResolveError::TypeAliasArity {
                span: self.file_span(cx, sf.1),
                name: def_id.name,
                expected,
                actual,
            });
            return Some(Ty::TyUnit);
        }

        let args = args
            .into_iter()
            .map(|arg| self.resolve_surface_type_in_module(arg, cx, local))
            .collect::<Vec<_>>();
        let mut ty = alias.ty;
        for (param, arg) in alias.params.iter().zip(args) {
            ty = ty.substitute(*param, &arg);
        }

        Some(ty)
    }

    fn resolve_tycon_id(&mut self, r: &NameRef, span: FileSpan, cx: &ModuleContext) -> TyConId {
        match self.resolve_tycon_defid(r, span, cx) {
            Some(def_id) => {
                self.record_resolution(span, def_id);
                let qualified_name = self.qualified_def_name(def_id);

                cx.builtins
                    .type_ids
                    .get(&def_id)
                    .copied()
                    .unwrap_or_else(|| self.names.tycon_id(&qualified_name))
            }
            None => {
                let unresolved = self.unresolved_name(r);
                self.names.tycon_id(&unresolved)
            }
        }
    }

    fn resolve_tycon_defid(
        &mut self,
        r: &NameRef,
        span: FileSpan,
        cx: &ModuleContext,
    ) -> Option<DefId> {
        match r.segments.as_slice() {
            [name] => {
                if let Some(sym) = cx.builtins.types.get(name) {
                    return Some(*sym);
                }

                if let Some(sym) = cx.local_api.types.get(name) {
                    return Some(*sym);
                }

                let mut candidates = Vec::new();

                for imp in &cx.imported_apis {
                    if let Some(sym) = imp.api.types.get(name)
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
                            ResolveNameKind::TypeConstructor,
                            self.symbol_text(*name),
                        );
                        None
                    }
                    _ => {
                        self.report_ambiguous_import(
                            span,
                            ResolveNameKind::TypeConstructor,
                            *name,
                            candidates,
                        );
                        None
                    }
                }
            }

            [_first, rest @ ..] => {
                let local_name = *rest.last()?;

                if cx.module.0.as_slice() == r.prefix() {
                    if let Some(sym) = cx.local_api.types.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(
                        span,
                        ResolveNameKind::TypeConstructor,
                        self.unresolved_name(r),
                    );
                    return None;
                }

                if let Some(imp) = self.imported_by_alias_or_module(r, cx) {
                    if let Some(sym) = imp.api.types.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(
                        span,
                        ResolveNameKind::TypeConstructor,
                        self.unresolved_name(r),
                    );
                    return None;
                }

                self.report_unknown_module(span, ModuleName(r.prefix().to_vec()));
                None
            }

            [] => solveig_base::ice!("empty type constructor name ref"),
        }
    }
    fn resolve_surface_type_in_module(
        &mut self,
        sf: &SpannedWrapper<SurfaceTy>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Ty {
        if let Some(ty) = self.try_expand_type_alias_application(sf, cx, local) {
            return ty;
        }

        match &sf.0 {
            SurfaceTy::Var(var_name) => Ty::TyVar(self.resolve_tyvar_symbol(*var_name)),

            SurfaceTy::Arr(lhs, rhs) => Ty::make_arr(
                self.resolve_surface_type_in_module(lhs, cx, local),
                self.resolve_surface_type_in_module(rhs, cx, local),
            ),

            SurfaceTy::ForAll(quants, body) => {
                self.push_tyvar_scope();
                let quantifiers = quants
                    .iter()
                    .map(|quant| (self.bind_tyvar_in_current_scope(quant.0), Kind::Star))
                    .collect::<Vec<_>>();
                let body = self.resolve_surface_type_in_module(body, cx, local);
                self.pop_tyvar_scope();
                Ty::make_forall_group(quantifiers, Vec::new(), body)
            }

            SurfaceTy::Qualified(preds, body) => Ty::make_qualified(
                preds
                    .iter()
                    .map(|pred| self.resolve_surface_predicate(pred, cx, local))
                    .collect(),
                self.resolve_surface_type_in_module(body, cx, local),
            ),

            SurfaceTy::Unit => Ty::TyUnit,

            SurfaceTy::Con(con) => {
                Ty::make_con_id(self.resolve_tycon_id(&con.0, self.file_span(cx, con.1), cx))
            }

            SurfaceTy::App(lhs, rhs) => Ty::make_app(
                self.resolve_surface_type_in_module(lhs, cx, local),
                self.resolve_surface_type_in_module(rhs, cx, local),
            ),
        }
    }
    fn resolve_surface_scheme(
        &mut self,
        ty: &SpannedWrapper<SurfaceTy>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Scheme {
        self.push_tyvar_scope();
        let raw = self.resolve_surface_type_in_module(ty, cx, local);
        self.pop_tyvar_scope();
        let (quantifiers, predicates, body_ty) = raw.peel_scheme_like();

        self.canonicalize_scheme_classes(
            Scheme {
                quantifiers,
                predicates,
                ty: body_ty,
            },
            cx,
            local,
        )
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "signature resolution threads the module context pieces without hiding them in a temporary aggregate"
    )]
    fn resolve_signatures(
        &mut self,
        module_name: &ModuleName,
        parsed: &ParsedModule,
        cx: &ModuleContext,
        imported: &ResolverImports,
        local: &LocalResolverEnv,
        local_tycon_kinds: &FxHashMap<TyConId, Kind>,
        class_defs: &[ClassDefinition],
    ) -> Result<FxHashMap<DefId, Scheme>, KindError> {
        let mut func_schemes = FxHashMap::default();

        let mut sig_tycon_env = imported.tycon_kind_ids.clone();
        sig_tycon_env.extend(local_tycon_kinds.clone());

        let sig_class_env = Self::class_param_kind_env(imported, class_defs);

        for defn in &parsed.defs {
            let SurfaceDefinition::SigDef {
                name: spanned_name,
                ty,
                ..
            } = &defn.0
            else {
                continue;
            };

            let (name, _span, _) = spanned_name;
            let fn_id = self.local_def_id(cx, *name);
            let fn_name = self.qualified_def_name(fn_id);
            let raw_scheme = self.resolve_surface_scheme(ty, cx, local);

            trace_predicates(
                self.session,
                module_name,
                &format!("signature {fn_name} predicates"),
                &raw_scheme.predicates,
            );

            if !self.errors.is_empty() {
                continue;
            }

            let inf =
                SignatureKindInferencer::with_envs(sig_tycon_env.clone(), sig_class_env.clone());
            let info = inf
                .infer_scheme(&raw_scheme)
                .map_err(|error| error.with_span(self.file_span(cx, ty.1)))?;

            func_schemes.insert(fn_id, info.scheme.clone());
        }

        Ok(func_schemes)
    }
}
