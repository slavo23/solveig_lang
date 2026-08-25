use super::*;

#[derive(Debug, Clone)]
pub struct EffectiveImport {
    pub qualifier: Symbol,
    pub qualifies: bool,
    pub accepts_module_name: bool,
    pub module: ModuleName,
    pub selection: ModuleSelection,
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
            qualifier: imp
                .alias
                .map_or_else(|| imp.module.default_alias(), |alias| alias.0),
            qualifies: true,
            accepts_module_name: imp.alias.is_none(),
            module: imp.module.clone(),
            selection: imp.selection.clone(),
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
            qualifier: implicit.default_alias(),
            qualifies: false,
            accepts_module_name: true,
            module: implicit.clone(),
            selection: ModuleSelection::All,
            span: FileSpan::new(file, Span::new(0, 0)),
        });
    }

    out
}
