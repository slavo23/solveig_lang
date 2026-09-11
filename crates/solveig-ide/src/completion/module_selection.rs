//! CST-directed completion for module header selections.
//! Resolver API projections define which entities each directive can legally name.

use super::*;

pub(super) fn completion_context(
    artifacts: &CompilationArtifacts,
    target: position::PositionTarget,
) -> CompletionContext {
    if let Some(index) = position::position_index_for_target(artifacts, target) {
        let tree = index.syntax_tree();
        let mut current = Some(target.node);
        while let Some(node) = current {
            let kind = tree.node(node).kind();
            if matches!(
                kind,
                SyntaxKind::ImportDecl
                    | SyntaxKind::ExportDecl
                    | SyntaxKind::PrivateDecl
                    | SyntaxKind::ReexportDecl
            ) && tree.tokens().iter().any(|token| {
                let node_span = tree.node(node).span().to_usize_range();
                token.kind == solveig_syntax::lexer::Token::LParen
                    && node_span.start <= token.start()
                    && token.start() <= target.offset
                    && token.end() <= node_span.end
            }) {
                return CompletionContext::ModuleSelection { kind, node };
            }
            current = tree.parent(node);
        }
    }

    let file = target.file;
    let offset = target.offset;
    let Some(source) = artifacts.source_files.get(file.0 as usize) else {
        return CompletionContext::Scope { qualifier: None };
    };
    let offset = offset.min(source.src.len());
    let Some(prefix) = source.src.get(..offset) else {
        return CompletionContext::Scope { qualifier: None };
    };
    let start = prefix
        .rfind(|ch: char| !completion_path_char(ch))
        .map_or(0, |index| index + 1);
    let Some(path) = source.src.get(start..offset) else {
        return CompletionContext::Scope { qualifier: None };
    };
    let qualifier = path.rfind('.').and_then(|dot| {
        let qualifier = path.get(..dot)?;
        (!qualifier.is_empty()).then(|| qualifier.to_string())
    });

    CompletionContext::Scope { qualifier }
}

pub(super) fn module_selection_items(
    session: &Session,
    snapshot: &CompilationSnapshot,
    candidates: &[SourceCompletionCandidate],
    file: FileId,
    offset: u32,
    kind: SyntaxKind,
    node: CstId,
) -> Vec<SourceCompletionItem> {
    let Some((module_name, module_file)) = snapshot
        .project
        .modules
        .iter()
        .find(|(_, module)| module.parsed.file == file)
    else {
        return Vec::new();
    };
    match kind {
        SyntaxKind::ExportDecl | SyntaxKind::PrivateDecl => {
            let items = snapshot.apis.get(module_name).map_or_else(
                || local_module_entity_items(candidates, file, offset),
                |local_api| {
                    let visible_api = local_api.local_view();
                    let visible_typed_api = snapshot
                        .typed_apis
                        .get(module_name)
                        .map(|api| api.selected_by_api(&visible_api));
                    module_entity_completion_items(
                        session,
                        &snapshot.names,
                        &visible_api,
                        visible_typed_api.as_ref(),
                        &snapshot.typed_apis,
                    )
                },
            );
            let entities = if kind == SyntaxKind::ExportDecl {
                module_file
                    .parsed
                    .directives
                    .exports
                    .iter()
                    .find(|directive| directive.cst_id == node)
                    .map(|directive| directive.entities.as_slice())
            } else {
                module_file
                    .parsed
                    .directives
                    .private
                    .iter()
                    .find(|directive| directive.cst_id == node)
                    .map(|directive| directive.entities.as_slice())
            };
            without_selected_entities(session, items, entities.unwrap_or_default())
        }
        SyntaxKind::ImportDecl => module_file
            .parsed
            .imports
            .iter()
            .find(|import| import.cst_id == node)
            .map_or_else(Vec::new, |import| {
                let items = imported_module_completion_items(
                    session,
                    snapshot,
                    module_file.package,
                    &import.module,
                );
                without_selected_entities(session, items, selection_entities(&import.selection))
            }),
        SyntaxKind::ReexportDecl => module_file
            .parsed
            .directives
            .reexports
            .iter()
            .find(|reexport| reexport.cst_id == node)
            .map_or_else(Vec::new, |reexport| {
                let items = imported_module_completion_items(
                    session,
                    snapshot,
                    module_file.package,
                    &reexport.module,
                );
                without_selected_entities(session, items, selection_entities(&reexport.selection))
            }),
        _ => Vec::new(),
    }
}

fn local_module_entity_items(
    candidates: &[SourceCompletionCandidate],
    file: FileId,
    offset: u32,
) -> Vec<SourceCompletionItem> {
    candidates
        .iter()
        .filter(|candidate| candidate.is_visible_at(file, offset, None))
        .map(SourceCompletionCandidate::item)
        .filter(|item| {
            matches!(
                item.kind(),
                SourceCompletionItemKind::Function
                    | SourceCompletionItemKind::Type
                    | SourceCompletionItemKind::Class
            )
        })
        .cloned()
        .collect()
}

fn selection_entities(selection: &ModuleSelection) -> &[ModuleEntity] {
    match selection {
        ModuleSelection::All => &[],
        ModuleSelection::Only(entities) => entities,
    }
}

fn without_selected_entities(
    session: &Session,
    mut items: Vec<SourceCompletionItem>,
    entities: &[ModuleEntity],
) -> Vec<SourceCompletionItem> {
    items.retain(|item| {
        !entities
            .iter()
            .any(|entity| &*session.str(entity.name.0) == item.label())
    });
    items
}

fn imported_module_completion_items(
    session: &Session,
    snapshot: &CompilationSnapshot,
    requesting_package: PackageId,
    module_name: &ModuleName,
) -> Vec<SourceCompletionItem> {
    let Some(source_api) = snapshot.apis.get(module_name) else {
        return Vec::new();
    };
    let visible_api = source_api.visible_to(requesting_package);
    let visible_typed_api = snapshot
        .typed_apis
        .get(module_name)
        .map(|api| api.visible_to(source_api, requesting_package));

    module_entity_completion_items(
        session,
        &snapshot.names,
        &visible_api,
        visible_typed_api.as_ref(),
        &snapshot.typed_apis,
    )
}

fn completion_path_char(ch: char) -> bool {
    ch == '.' || ch == '_' || ch.is_ascii_alphanumeric()
}
