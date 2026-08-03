use super::*;
use crate::symbol_artifacts::collect_source_symbols;

pub(super) struct RetainedIdeArtifacts {
    pub(super) docs: FxHashMap<DefId, DocComment>,
    pub(super) source_signatures: FxHashMap<DefId, String>,
    pub(super) source_type_var_hovers: Vec<HoverInfo>,
    pub(super) declaration_spans: Vec<(FileSpan, DefId)>,
    pub(super) module_display_names: FxHashMap<ModuleId, String>,
    pub(super) module_paths: FxHashMap<ModuleId, PathBuf>,
    pub(super) module_imports: Vec<(FileSpan, ModuleId)>,
    pub(super) import_name_resolutions: Vec<(ModuleId, FileSpan, DefId)>,
    pub(super) source_symbols: Vec<SourceSymbol>,
}

pub(super) fn collect_retained_ide_artifacts(
    session: &Session,
    names: &Names,
    project: &Project,
    typed_apis: &FxHashMap<ModuleName, SharedTypedModuleApi>,
) -> RetainedIdeArtifacts {
    let docs = typed_apis
        .values()
        .flat_map(|api| api.docs.iter().map(|(id, doc)| (*id, doc.clone())))
        .collect();

    RetainedIdeArtifacts {
        docs,
        source_signatures: collect_hover_source_signatures(session, project),
        source_type_var_hovers: collect_source_type_var_hovers(session, names, project, typed_apis),
        declaration_spans: collect_hover_declaration_spans(project),
        module_display_names: collect_hover_module_display_names(session, project),
        module_paths: collect_hover_module_paths(project),
        module_imports: collect_hover_module_imports(project),
        import_name_resolutions: collect_hover_import_name_resolutions(project, typed_apis),
        source_symbols: collect_source_symbols(session, project),
    }
}

fn collect_hover_declaration_spans(project: &Project) -> Vec<(FileSpan, DefId)> {
    let mut out = Vec::new();

    for (module_name, module_file) in &project.modules {
        let module_id = project
            .module_table
            .id(module_name)
            .expect("ICE: hover declaration collection referenced unknown module");

        for defn in &module_file.parsed.defs {
            match &defn.0 {
                SurfaceDefinition::FunDef { name, .. }
                | SurfaceDefinition::SigDef { name, .. }
                | SurfaceDefinition::DataDef { name, .. }
                | SurfaceDefinition::TypeAliasDef { name, .. } => {
                    out.push((
                        FileSpan::new(module_file.parsed.file, name.1),
                        DefId {
                            module: module_id,
                            name: name.0,
                        },
                    ));
                }
                SurfaceDefinition::ClassDef { name, methods, .. } => {
                    out.push((
                        FileSpan::new(module_file.parsed.file, name.1),
                        DefId {
                            module: module_id,
                            name: name.0,
                        },
                    ));

                    for method in methods {
                        out.push((
                            FileSpan::new(module_file.parsed.file, method.name.1),
                            DefId {
                                module: module_id,
                                name: method.name.0,
                            },
                        ));
                    }
                }
                SurfaceDefinition::FixityDef { .. } | SurfaceDefinition::InstanceDef { .. } => {}
            }

            if let SurfaceDefinition::DataDef { constructors, .. } = &defn.0 {
                for constructor in constructors {
                    out.push((
                        FileSpan::new(module_file.parsed.file, constructor.0.name.1),
                        DefId {
                            module: module_id,
                            name: constructor.0.name.0,
                        },
                    ));
                }
            }
        }
    }

    out.sort_by_key(|(span, def)| {
        (
            span.file,
            span.span.start,
            span.span.end,
            def.module,
            def.name,
        )
    });
    out
}

#[derive(Clone)]
struct SourceKindEnvs {
    tycons: FxHashMap<TyConId, Kind>,
    classes: FxHashMap<ClassId, Vec<Kind>>,
}

fn collect_source_type_var_hovers(
    session: &Session,
    names: &Names,
    project: &Project,
    typed_apis: &FxHashMap<ModuleName, SharedTypedModuleApi>,
) -> Vec<HoverInfo> {
    let mut out = Vec::new();
    let kind_envs = merged_source_kind_envs(typed_apis);

    for (module_name, module_file) in &project.modules {
        let Some(module_id) = project.module_table.id(module_name) else {
            continue;
        };
        let Some(typed_api) = typed_apis.get(module_name) else {
            continue;
        };

        for defn in &module_file.parsed.defs {
            match &defn.0 {
                SurfaceDefinition::SigDef { name, ty, .. } => {
                    let def_id = DefId {
                        module: module_id,
                        name: name.0,
                    };
                    if let Some(scheme) = typed_api.term_schemes.get(&def_id) {
                        collect_type_var_hovers_in_ty(
                            session,
                            names,
                            module_file.parsed.file,
                            ty,
                            &[],
                            &scheme.quantifiers,
                            &mut out,
                        );
                    }
                }
                SurfaceDefinition::DataDef {
                    name,
                    ty_params,
                    constructors,
                    ..
                } => {
                    let def_id = DefId {
                        module: module_id,
                        name: name.0,
                    };
                    if let Some(data_def) = data_def_for_source(session, typed_api, name.0, def_id)
                    {
                        push_type_param_hovers(
                            session,
                            names,
                            module_file.parsed.file,
                            ty_params,
                            &data_def.ty_params,
                            &mut out,
                        );
                        for constructor in constructors {
                            for field in &constructor.0.params {
                                collect_type_var_hovers_in_ty(
                                    session,
                                    names,
                                    module_file.parsed.file,
                                    field,
                                    &data_def.ty_params,
                                    &[],
                                    &mut out,
                                );
                            }
                        }
                    }
                }
                SurfaceDefinition::TypeAliasDef {
                    name,
                    ty_params,
                    ty,
                    ..
                } => {
                    let def_id = DefId {
                        module: module_id,
                        name: name.0,
                    };
                    if let Some(alias) = typed_api.type_aliases.get(&def_id) {
                        let params = infer_type_alias_param_kinds(alias, &kind_envs);
                        push_type_param_hovers(
                            session,
                            names,
                            module_file.parsed.file,
                            ty_params,
                            &params,
                            &mut out,
                        );
                        collect_type_var_hovers_in_ty(
                            session,
                            names,
                            module_file.parsed.file,
                            ty,
                            &params,
                            &[],
                            &mut out,
                        );
                    }
                }
                SurfaceDefinition::ClassDef {
                    name,
                    ty_params,
                    superclasses,
                    methods,
                    ..
                } => {
                    let def_id = DefId {
                        module: module_id,
                        name: name.0,
                    };
                    let Some(class_def) = class_def_for_source(session, typed_api, name.0, def_id)
                    else {
                        continue;
                    };

                    push_type_param_hovers(
                        session,
                        names,
                        module_file.parsed.file,
                        ty_params,
                        &class_def.params,
                        &mut out,
                    );

                    for superclass in superclasses {
                        collect_type_var_hovers_in_predicate(
                            session,
                            names,
                            module_file.parsed.file,
                            superclass,
                            &class_def.params,
                            &[],
                            &mut out,
                        );
                    }

                    for method in methods {
                        let def_id = DefId {
                            module: module_id,
                            name: method.name.0,
                        };
                        let source_method_name = session.str(method.name.0).to_string();
                        if let Some(method_def) = class_def.methods.iter().find(|method_def| {
                            method_def.id == def_id || method_def.short_name == source_method_name
                        }) {
                            collect_type_var_hovers_in_ty(
                                session,
                                names,
                                module_file.parsed.file,
                                &method.ty,
                                &class_def.params,
                                &method_def.scheme.quantifiers,
                                &mut out,
                            );
                        }
                    }
                }
                SurfaceDefinition::InstanceDef { context, args, .. } => {
                    if let Some(instance) = typed_api
                        .instance_defs
                        .iter()
                        .find(|instance| instance.span == defn.1.to_usize_range())
                    {
                        let params = infer_instance_type_var_kinds(instance, &kind_envs);
                        for predicate in context {
                            collect_type_var_hovers_in_predicate(
                                session,
                                names,
                                module_file.parsed.file,
                                predicate,
                                &[],
                                &params,
                                &mut out,
                            );
                        }
                        for arg in args {
                            collect_type_var_hovers_in_ty(
                                session,
                                names,
                                module_file.parsed.file,
                                arg,
                                &[],
                                &params,
                                &mut out,
                            );
                        }
                    }
                }
                SurfaceDefinition::FunDef { .. } | SurfaceDefinition::FixityDef { .. } => {}
            }
        }
    }

    out.sort_by_key(|info| {
        let span = info.file_span();
        (span.file, span.span.start, span.span.end)
    });
    out
}

struct SourceTypeVarHoverCollector<'a, 'out> {
    session: &'a Session,
    file: FileId,
    globals: FxHashMap<String, Kind>,
    forall_kinds: FxHashMap<String, std::collections::VecDeque<Kind>>,
    scopes: Vec<FxHashMap<String, Kind>>,
    out: &'out mut Vec<HoverInfo>,
}

impl<'a, 'out> SourceTypeVarHoverCollector<'a, 'out> {
    fn new(
        session: &'a Session,
        names: &Names,
        file: FileId,
        outer: &[(TyVarId, Kind)],
        quantifiers: &[(TyVarId, Kind)],
        out: &'out mut Vec<HoverInfo>,
    ) -> Self {
        let mut globals = FxHashMap::default();
        let mut forall_kinds = FxHashMap::<String, std::collections::VecDeque<Kind>>::default();

        for (id, kind) in outer.iter().chain(quantifiers.iter()) {
            globals
                .entry(names.tyvar_display_name(*id))
                .or_insert_with(|| kind.clone());
        }

        for (id, kind) in quantifiers {
            forall_kinds
                .entry(names.tyvar_display_name(*id))
                .or_default()
                .push_back(kind.clone());
        }

        Self {
            session,
            file,
            globals,
            forall_kinds,
            scopes: Vec::new(),
            out,
        }
    }

    fn collect_ty(&mut self, ty: &Spanned<SurfaceTy>) {
        match &ty.0 {
            SurfaceTy::Var(symbol) => {
                let name = self.session.str(*symbol).to_string();
                if let Some(kind) = self.kind_for_name(&name).cloned() {
                    self.push_hover(ty.1, &name, &kind);
                }
            }
            SurfaceTy::ForAll(params, body) => {
                let mut scope = FxHashMap::default();
                for param in params {
                    let name = self.session.str(param.0).to_string();
                    let kind = self
                        .pop_forall_kind(&name)
                        .or_else(|| self.kind_for_name(&name).cloned())
                        .unwrap_or(Kind::Star);
                    self.push_hover(param.1, &name, &kind);
                    scope.insert(name, kind);
                }

                self.scopes.push(scope);
                self.collect_ty(body);
                self.scopes.pop();
            }
            SurfaceTy::Arr(lhs, rhs) | SurfaceTy::App(lhs, rhs) => {
                self.collect_ty(lhs);
                self.collect_ty(rhs);
            }
            SurfaceTy::Qualified(predicates, body) => {
                for predicate in predicates {
                    self.collect_predicate(predicate);
                }
                self.collect_ty(body);
            }
            SurfaceTy::Unit | SurfaceTy::Con(_) => {}
        }
    }

    fn collect_predicate(&mut self, predicate: &Spanned<SurfacePredicate>) {
        for arg in &predicate.0.args {
            self.collect_ty(arg);
        }
    }

    fn kind_for_name(&self, name: &str) -> Option<&Kind> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .or_else(|| self.globals.get(name))
    }

    fn pop_forall_kind(&mut self, name: &str) -> Option<Kind> {
        self.forall_kinds.get_mut(name)?.pop_front()
    }

    fn push_hover(&mut self, span: Span, name: &str, kind: &Kind) {
        self.out.push(HoverInfo::new(
            self.file,
            span.to_usize_range(),
            format!("{name} : {}", kind.pretty()),
            None,
        ));
    }
}

fn collect_type_var_hovers_in_ty(
    session: &Session,
    names: &Names,
    file: FileId,
    ty: &Spanned<SurfaceTy>,
    outer: &[(TyVarId, Kind)],
    quantifiers: &[(TyVarId, Kind)],
    out: &mut Vec<HoverInfo>,
) {
    SourceTypeVarHoverCollector::new(session, names, file, outer, quantifiers, out).collect_ty(ty);
}

fn collect_type_var_hovers_in_predicate(
    session: &Session,
    names: &Names,
    file: FileId,
    predicate: &Spanned<SurfacePredicate>,
    outer: &[(TyVarId, Kind)],
    quantifiers: &[(TyVarId, Kind)],
    out: &mut Vec<HoverInfo>,
) {
    SourceTypeVarHoverCollector::new(session, names, file, outer, quantifiers, out)
        .collect_predicate(predicate);
}

fn push_type_param_hovers(
    session: &Session,
    names: &Names,
    file: FileId,
    params: &[Spanned<Symbol>],
    quantifiers: &[(TyVarId, Kind)],
    out: &mut Vec<HoverInfo>,
) {
    let kind_by_name = quantifier_kinds_by_source_name(names, quantifiers);

    for param in params {
        let name = session.str(param.0).to_string();
        if let Some(kind) = kind_by_name.get(&name) {
            out.push(HoverInfo::new(
                file,
                param.1.to_usize_range(),
                format!("{name} : {}", kind.pretty()),
                None,
            ));
        }
    }
}

fn data_def_for_source<'a>(
    session: &Session,
    api: &'a TypedModuleApi,
    source_name: Symbol,
    def_id: DefId,
) -> Option<&'a DataDefinition> {
    api.type_ids
        .get(&def_id)
        .and_then(|tycon_id| api.tycon_table.get(*tycon_id))
        .and_then(data_def_from_tycon_info)
        .or_else(|| {
            let short_name = session.str(source_name);
            api.tycon_table.iter().find_map(|(_, info)| {
                data_def_from_tycon_info(info).filter(|data_def| data_def.short_name == *short_name)
            })
        })
}

fn data_def_from_tycon_info(info: &TyConInfo) -> Option<&DataDefinition> {
    match &info.definition {
        TyConDefinition::Data(data_def) => Some(data_def),
        TyConDefinition::Primitive => None,
    }
}

fn class_def_for_source<'a>(
    session: &Session,
    api: &'a TypedModuleApi,
    source_name: Symbol,
    def_id: DefId,
) -> Option<&'a ClassDefinition> {
    api.class_defs.get(&def_id).or_else(|| {
        let short_name = session.str(source_name);
        api.class_defs
            .values()
            .find(|class_def| class_def.short_name == *short_name)
    })
}

fn infer_type_alias_param_kinds(
    alias: &TypeAlias,
    kind_envs: &SourceKindEnvs,
) -> Vec<(TyVarId, Kind)> {
    let scheme = Scheme {
        quantifiers: alias
            .params
            .iter()
            .map(|param| (*param, Kind::Star))
            .collect(),
        predicates: Vec::new(),
        ty: alias.ty.clone(),
    };

    solveig_check::signature_infer::SignatureKindInferencer::with_envs(
        kind_envs.tycons.clone(),
        kind_envs.classes.clone(),
    )
    .infer_scheme(&scheme)
    .map_or_else(
        |_| {
            alias
                .params
                .iter()
                .map(|param| (*param, Kind::Star))
                .collect()
        },
        |info| info.scheme.quantifiers,
    )
}

fn infer_instance_type_var_kinds(
    instance: &InstanceDefinition<TcMetadata, Range<usize>>,
    kind_envs: &SourceKindEnvs,
) -> Vec<(TyVarId, Kind)> {
    let mut predicates = instance.context.clone();
    predicates.push(instance.head.clone());
    let scheme = Scheme {
        quantifiers: Vec::new(),
        predicates,
        ty: Ty::TyUnit,
    };

    solveig_check::signature_infer::SignatureKindInferencer::with_envs(
        kind_envs.tycons.clone(),
        kind_envs.classes.clone(),
    )
    .infer_scheme(&scheme)
    .map_or_else(
        |_| {
            let mut seen = FxHashSet::default();
            let mut out = Vec::new();
            for predicate in instance
                .context
                .iter()
                .chain(std::iter::once(&instance.head))
            {
                for arg in &predicate.args {
                    collect_free_tyvar_kinds(arg, &mut Vec::new(), &mut seen, &mut out);
                }
            }
            out
        },
        |info| info.scheme.quantifiers,
    )
}

fn collect_free_tyvar_kinds(
    ty: &Ty,
    bound: &mut Vec<TyVarId>,
    seen: &mut FxHashSet<TyVarId>,
    out: &mut Vec<(TyVarId, Kind)>,
) {
    match ty {
        Ty::TyUnit | Ty::TyCon(_) | Ty::TyExVar(_) => {}
        Ty::TyVar(id) => {
            if !bound.contains(id) && seen.insert(*id) {
                out.push((*id, Kind::Star));
            }
        }
        Ty::TyArr(lhs, rhs) | Ty::TyApp(lhs, rhs) => {
            collect_free_tyvar_kinds(lhs, bound, seen, out);
            collect_free_tyvar_kinds(rhs, bound, seen, out);
        }
        Ty::TyForAll(binders, predicates, body) => {
            for (id, _) in binders {
                bound.push(*id);
            }
            for predicate in predicates {
                for arg in &predicate.args {
                    collect_free_tyvar_kinds(arg, bound, seen, out);
                }
            }
            collect_free_tyvar_kinds(body, bound, seen, out);
            for _ in binders {
                bound.pop();
            }
        }
    }
}

fn merged_source_kind_envs(
    typed_apis: &FxHashMap<ModuleName, SharedTypedModuleApi>,
) -> SourceKindEnvs {
    let mut tycons = FxHashMap::default();
    let mut classes = FxHashMap::default();

    for api in typed_apis.values() {
        for (id, info) in api.tycon_table.iter() {
            tycons.insert(id, info.kind.clone());
        }
        for (def_id, kind) in &api.type_kinds {
            if let Some(id) = api.type_ids.get(def_id) {
                tycons.insert(*id, kind.clone());
            }
        }
        for (id, info) in api.class_table.iter() {
            classes.insert(
                id,
                info.definition
                    .params
                    .iter()
                    .map(|(_, kind)| kind.clone())
                    .collect(),
            );
        }
        for class_def in api.class_defs.values() {
            classes.insert(
                class_def.id,
                class_def
                    .params
                    .iter()
                    .map(|(_, kind)| kind.clone())
                    .collect(),
            );
        }
    }

    SourceKindEnvs { tycons, classes }
}

fn quantifier_kinds_by_source_name(
    names: &Names,
    quantifiers: &[(TyVarId, Kind)],
) -> FxHashMap<String, Kind> {
    let mut out = FxHashMap::default();

    for (id, kind) in quantifiers {
        out.entry(names.tyvar_display_name(*id))
            .or_insert_with(|| kind.clone());
    }

    out
}

fn collect_hover_source_signatures(
    session: &Session,
    project: &Project,
) -> FxHashMap<DefId, String> {
    let source_map = session.source_map();
    let mut out = FxHashMap::default();

    for (module_name, module_file) in &project.modules {
        let Some(module_id) = project.module_table.id(module_name) else {
            continue;
        };
        let source = &source_map.lookup(module_file.parsed.file).src;

        for defn in &module_file.parsed.defs {
            match &defn.0 {
                SurfaceDefinition::SigDef { name, .. } => {
                    if let Some(signature) = source_signature_text(source, defn.1) {
                        out.insert(
                            DefId {
                                module: module_id,
                                name: name.0,
                            },
                            signature,
                        );
                    }
                }
                SurfaceDefinition::ClassDef { methods, .. } => {
                    for method in methods {
                        let span = Span::new(method.name.1.start, method.ty.1.end);
                        if let Some(signature) = source_signature_text(source, span) {
                            out.insert(
                                DefId {
                                    module: module_id,
                                    name: method.name.0,
                                },
                                signature,
                            );
                        }
                    }
                }
                SurfaceDefinition::FunDef { .. }
                | SurfaceDefinition::DataDef { .. }
                | SurfaceDefinition::TypeAliasDef { .. }
                | SurfaceDefinition::FixityDef { .. }
                | SurfaceDefinition::InstanceDef { .. } => {}
            }
        }
    }

    out
}

fn source_signature_text(source: &str, span: Span) -> Option<String> {
    let text = source.get(span.to_usize_range())?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn collect_hover_module_display_names(
    session: &Session,
    project: &Project,
) -> FxHashMap<ModuleId, String> {
    project
        .modules
        .keys()
        .filter_map(|module_name| {
            let module_id = project.module_table.id(module_name)?;
            Some((module_id, module_name.as_string(session)))
        })
        .collect()
}

fn collect_hover_module_paths(project: &Project) -> FxHashMap<ModuleId, PathBuf> {
    project
        .modules
        .iter()
        .filter_map(|(module_name, module_file)| {
            let module_id = project.module_table.id(module_name)?;
            Some((module_id, module_file.path.clone()))
        })
        .collect()
}

fn collect_hover_module_imports(project: &Project) -> Vec<(FileSpan, ModuleId)> {
    let mut out = Vec::new();

    for module_file in project.modules.values() {
        for import in &module_file.imports {
            if let Some(module_id) = project.module_table.id(&import.module) {
                out.push((
                    FileSpan::new(module_file.parsed.file, import.span),
                    module_id,
                ));
            }
        }
    }

    out.sort_by_key(|(span, module)| (span.file, span.span.start, span.span.end, *module));
    out
}

fn collect_hover_import_name_resolutions(
    project: &Project,
    typed_apis: &FxHashMap<ModuleName, SharedTypedModuleApi>,
) -> Vec<(ModuleId, FileSpan, DefId)> {
    let mut out = Vec::new();

    for (module_name, module_file) in &project.modules {
        let Some(module_id) = project.module_table.id(module_name) else {
            continue;
        };

        for import in &module_file.imports {
            let Some(imported_api) = typed_apis.get(&import.module) else {
                continue;
            };

            for selected_name in &import.selected_names {
                for def_id in typed_import_def_ids_for_name(imported_api, selected_name.0) {
                    out.push((
                        module_id,
                        FileSpan::new(module_file.parsed.file, selected_name.1),
                        def_id,
                    ));
                }
            }
        }
    }

    out.sort_by_key(|(module, span, def)| {
        (
            *module,
            span.file,
            span.span.start,
            span.span.end,
            def.module,
            def.name,
        )
    });
    out.dedup();
    out
}

fn typed_import_def_ids_for_name(api: &TypedModuleApi, name: Symbol) -> Vec<DefId> {
    let mut out = Vec::new();

    out.extend(
        api.term_schemes
            .keys()
            .copied()
            .filter(|def| def.name == name),
    );
    out.extend(
        api.type_kinds
            .keys()
            .copied()
            .filter(|def| def.name == name),
    );
    out.extend(
        api.type_aliases
            .keys()
            .copied()
            .filter(|def| def.name == name),
    );
    out.extend(
        api.constructor_types
            .keys()
            .copied()
            .filter(|def| def.name == name),
    );
    out.extend(
        api.class_defs
            .keys()
            .copied()
            .filter(|def| def.name == name),
    );
    out.extend(api.class_defs.values().flat_map(|class_def| {
        class_def
            .methods
            .iter()
            .map(|method| method.id)
            .filter(move |def| def.name == name)
    }));

    out.sort();
    out.dedup();
    out
}
