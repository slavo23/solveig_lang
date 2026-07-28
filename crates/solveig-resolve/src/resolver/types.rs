impl<'a> Resolver<'a> {
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
                            self.symbol_text(*name).to_string(),
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
                let local_name = *rest
                    .last()
                    .expect("ICE: qualified type constructor name must have a last segment");

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

            [] => panic!("ICE: empty type constructor name ref"),
        }
    }
    fn resolve_surface_type_in_module(
        &mut self,
        sf: &SpannedWrapper<SurfaceTy>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> TypeExpr {
        match &sf.0 {
            SurfaceTy::Var(var_name) => TypeExpr::TyVar(self.resolve_tyvar_symbol(*var_name)),

            SurfaceTy::Arr(lhs, rhs) => TypeExpr::make_arr(
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
                TypeExpr::make_forall_group(quantifiers, Vec::new(), body)
            }

            SurfaceTy::Qualified(preds, body) => TypeExpr::make_qualified(
                preds
                    .iter()
                    .map(|pred| self.resolve_surface_predicate(pred, cx, local))
                    .collect(),
                self.resolve_surface_type_in_module(body, cx, local),
            ),

            SurfaceTy::Unit => TypeExpr::TyUnit,

            SurfaceTy::Con(con) => {
                TypeExpr::make_con_id(self.resolve_tycon_id(&con.0, self.file_span(cx, con.1), cx))
            }

            SurfaceTy::App(lhs, rhs) => TypeExpr::make_app(
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
    fn resolve_signatures(
        &mut self,
        module_name: &ModuleName,
        parsed: &ParsedModule,
        cx: &ModuleContext,
        imported: &ResolverImports,
        local: &LocalResolverEnv,
        local_tycon_kinds: &HashMap<TyConId, Kind>,
        class_defs: &[ClassDefinition],
    ) -> Result<HashMap<DefId, Scheme>, KindError> {
        let mut func_schemes = HashMap::new();

        let mut sig_tycon_env = imported.tycon_kind_ids.clone();
        sig_tycon_env.extend(local_tycon_kinds.clone());

        let sig_class_env = Self::class_param_kind_env(imported, class_defs);

        for defn in parsed.defs.iter() {
            let SurfaceDefinition::SigDef {
                name: spanned_name,
                ty,
            } = &defn.0
            else {
                continue;
            };

            let (name, _span) = spanned_name;
            let fn_id = self.local_def_id(cx, *name);
            let fn_name = self.qualified_def_name(fn_id);
            let raw_scheme = self.resolve_surface_scheme(ty, cx, local);

            trace_predicates(
                self.session,
                module_name,
                &format!("signature {} predicates", fn_name),
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
