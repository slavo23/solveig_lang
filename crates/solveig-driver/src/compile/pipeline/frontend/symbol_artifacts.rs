fn collect_source_symbols(session: &Session, project: &Project) -> Vec<SourceSymbol> {
    let mut out = Vec::new();

    for (module_name, module_file) in &project.modules {
        let module = module_name.as_string(session);
        let function_defs = module_file
            .parsed
            .defs
            .iter()
            .filter_map(|defn| match &defn.0 {
                SurfaceDefinition::FunDef { name, .. } => Some(name.0),
                _ => None,
            })
            .collect::<FxHashSet<_>>();

        for defn in &module_file.parsed.defs {
            if let Some(symbol) =
                source_symbol_for_definition(session, module_file, &module, defn, &function_defs)
            {
                out.push(symbol);
            }
        }
    }

    sort_symbols(&mut out);
    out
}

fn source_symbol_for_definition(
    session: &Session,
    module_file: &ModuleFile,
    module: &str,
    defn: &Spanned<SurfaceDefinition>,
    function_defs: &FxHashSet<Symbol>,
) -> Option<SourceSymbol> {
    match &defn.0 {
        SurfaceDefinition::FunDef { name, .. } => Some(SourceSymbol::new(
            session.str(name.0).to_string(),
            SourceSymbolKind::Function,
            FileSpan::new(module_file.parsed.file, defn.1),
            FileSpan::new(module_file.parsed.file, name.1),
            Some(module.to_string()),
            Vec::new(),
        )),
        SurfaceDefinition::SigDef { name, .. } if !function_defs.contains(&name.0) => {
            Some(SourceSymbol::new(
                session.str(name.0).to_string(),
                SourceSymbolKind::Function,
                FileSpan::new(module_file.parsed.file, defn.1),
                FileSpan::new(module_file.parsed.file, name.1),
                Some(module.to_string()),
                Vec::new(),
            ))
        }
        SurfaceDefinition::DataDef {
            name, constructors, ..
        } => Some(SourceSymbol::new(
            session.str(name.0).to_string(),
            SourceSymbolKind::Type,
            FileSpan::new(module_file.parsed.file, defn.1),
            FileSpan::new(module_file.parsed.file, name.1),
            Some(module.to_string()),
            constructor_symbols(session, module_file, name.0, constructors),
        )),
        SurfaceDefinition::TypeAliasDef { name, .. } => Some(SourceSymbol::new(
            session.str(name.0).to_string(),
            SourceSymbolKind::TypeAlias,
            FileSpan::new(module_file.parsed.file, defn.1),
            FileSpan::new(module_file.parsed.file, name.1),
            Some(module.to_string()),
            Vec::new(),
        )),
        SurfaceDefinition::ClassDef { name, methods, .. } => Some(SourceSymbol::new(
            session.str(name.0).to_string(),
            SourceSymbolKind::Class,
            FileSpan::new(module_file.parsed.file, defn.1),
            FileSpan::new(module_file.parsed.file, name.1),
            Some(module.to_string()),
            method_symbols(session, module_file, name.0, methods),
        )),
        SurfaceDefinition::FixityDef { .. }
        | SurfaceDefinition::InstanceDef { .. }
        | SurfaceDefinition::SigDef { .. } => None,
    }
}

fn constructor_symbols(
    session: &Session,
    module_file: &ModuleFile,
    type_name: Symbol,
    constructors: &[Spanned<SurfaceConstructor>],
) -> Vec<SourceSymbol> {
    let container = format!(
        "{}.{}",
        module_file.name.as_string(session),
        session.str(type_name)
    );
    let mut out = constructors
        .iter()
        .map(|constructor| {
            SourceSymbol::new(
                session.str(constructor.0.name.0).to_string(),
                SourceSymbolKind::Constructor,
                FileSpan::new(module_file.parsed.file, constructor.1),
                FileSpan::new(module_file.parsed.file, constructor.0.name.1),
                Some(container.clone()),
                Vec::new(),
            )
        })
        .collect::<Vec<_>>();

    sort_symbols(&mut out);
    out
}

fn method_symbols(
    session: &Session,
    module_file: &ModuleFile,
    class_name: Symbol,
    methods: &[ClassMethod],
) -> Vec<SourceSymbol> {
    let container = format!(
        "{}.{}",
        module_file.name.as_string(session),
        session.str(class_name)
    );
    let mut out = methods
        .iter()
        .map(|method| {
            SourceSymbol::new(
                session.str(method.name.0).to_string(),
                SourceSymbolKind::Method,
                FileSpan::new(
                    module_file.parsed.file,
                    solveig_base::Span::new(method.name.1.start, method.ty.1.end),
                ),
                FileSpan::new(module_file.parsed.file, method.name.1),
                Some(container.clone()),
                Vec::new(),
            )
        })
        .collect::<Vec<_>>();

    sort_symbols(&mut out);
    out
}

fn sort_symbols(symbols: &mut [SourceSymbol]) {
    symbols.sort_by_key(|symbol| {
        let span = symbol.selection_span();
        (span.file, span.span.start, span.span.end, symbol.name().to_string())
    });
}
