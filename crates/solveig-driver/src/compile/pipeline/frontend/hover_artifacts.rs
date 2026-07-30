struct RetainedFrontendArtifacts {
    docs: FxHashMap<DefId, DocComment>,
    source_signatures: FxHashMap<DefId, String>,
    declaration_spans: Vec<(FileSpan, DefId)>,
    module_display_names: FxHashMap<ModuleId, String>,
    module_paths: FxHashMap<ModuleId, PathBuf>,
    module_imports: Vec<(FileSpan, ModuleId)>,
    import_name_resolutions: Vec<(ModuleId, FileSpan, DefId)>,
    source_symbols: Vec<SourceSymbol>,
}

fn collect_retained_frontend_artifacts(
    session: &Session,
    project: &Project,
    typed_apis: &FxHashMap<ModuleName, SharedTypedModuleApi>,
) -> RetainedFrontendArtifacts {
    let docs = typed_apis
        .values()
        .flat_map(|api| api.docs.iter().map(|(id, doc)| (*id, doc.clone())))
        .collect();

    RetainedFrontendArtifacts {
        docs,
        source_signatures: collect_hover_source_signatures(session, project),
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

    out.sort_by_key(|(span, def)| (span.file, span.span.start, span.span.end, def.module, def.name));
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
                out.push((FileSpan::new(module_file.parsed.file, import.span), module_id));
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

    out.extend(api.term_schemes.keys().copied().filter(|def| def.name == name));
    out.extend(api.type_kinds.keys().copied().filter(|def| def.name == name));
    out.extend(api.type_aliases.keys().copied().filter(|def| def.name == name));
    out.extend(
        api.constructor_types
            .keys()
            .copied()
            .filter(|def| def.name == name),
    );
    out.extend(api.class_defs.keys().copied().filter(|def| def.name == name));
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
