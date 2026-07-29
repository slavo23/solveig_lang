//! Source-level document links for module imports.

use super::*;

pub(super) fn document_links_in_artifacts(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
) -> Vec<SourceDocumentLink> {
    let Some(file_id) = source_lookup::source_file_id_in_source_map(source_map, file) else {
        return Vec::new();
    };
    let source = &source_map.lookup(file_id).src;
    let mut out = Vec::new();

    for (span, module_id) in &artifacts.module_imports {
        if span.file != file_id {
            continue;
        }
        let Some(target) = artifacts.module_paths.get(module_id) else {
            continue;
        };
        let Some(display) = artifacts.module_display_names.get(module_id) else {
            continue;
        };
        let link_span = import_module_name_span(source, span.span, display).unwrap_or(span.span);

        out.push(
            SourceDocumentLink::new(FileSpan::new(file_id, link_span), target.clone())
                .with_tooltip(format!("Open module {display}")),
        );
    }

    out
}

fn import_module_name_span(source: &str, import_span: Span, module_name: &str) -> Option<Span> {
    let text = source.get(import_span.to_usize_range())?;
    let import_start = text.find("import")?;
    let search_start = import_start + "import".len();
    let name_start = text[search_start..].find(module_name)? + search_start;
    let name_end = name_start + module_name.len();

    Some(Span::new(
        u32::try_from(import_span.start as usize + name_start).ok()?,
        u32::try_from(import_span.start as usize + name_end).ok()?,
    ))
}
