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
