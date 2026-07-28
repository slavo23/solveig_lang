use super::*;

#[derive(Debug, Clone)]
pub struct EffectiveImport {
    pub alias: Symbol,
    pub module: ModuleName,
    pub spec: ImportSpec,
    pub span: FileSpan,
}

pub fn is_prelude_module(session: &Session, module: &ModuleName) -> bool {
    module.as_string(session).starts_with("prelude.")
}

fn implicit_prelude_modules(session: &Session, module: &ModuleName) -> Vec<ModuleName> {
    if is_prelude_module(session, module) {
        vec![]
    } else {
        vec![
            ModuleName(vec![session.intern("prelude"), session.intern("builtins")]),
            ModuleName(vec![session.intern("prelude"), session.intern("list")]),
            ModuleName(vec![
                session.intern("prelude"),
                session.intern("typeclasses"),
            ]),
        ]
    }
}

pub fn effective_imports(
    session: &Session,
    module: &ModuleName,
    explicit: &[ImportDecl],
    file: FileId,
) -> Vec<EffectiveImport> {
    let mut out = explicit
        .iter()
        .map(|imp| EffectiveImport {
            alias: imp.alias,
            module: imp.module.clone(),
            spec: imp.spec.clone(),
            span: FileSpan::new(file, imp.span),
        })
        .collect::<Vec<_>>();

    for implicit in implicit_prelude_modules(session, module) {
        out.push(EffectiveImport {
            alias: implicit.default_alias(),
            module: implicit,
            spec: ImportSpec::All,
            span: FileSpan::new(file, Span::new(0, 0)),
        });
    }

    out
}
