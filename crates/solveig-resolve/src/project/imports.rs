use super::*;

#[derive(Debug, Clone)]
pub struct EffectiveImport {
    pub alias: Symbol,
    pub module: ModuleName,
    pub spec: ImportSpec,
    pub span: FileSpan,
}

pub fn effective_imports(
    project: &Project,
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

    let imports_implicit_package = project
        .module_package(module)
        .and_then(|package| project.package_kind(package))
        != Some(PackageKind::Implicit);

    if !imports_implicit_package {
        return out;
    }

    for implicit in project.implicit_modules() {
        out.push(EffectiveImport {
            alias: implicit.default_alias(),
            module: implicit.clone(),
            spec: ImportSpec::All,
            span: FileSpan::new(file, Span::new(0, 0)),
        });
    }

    out
}
