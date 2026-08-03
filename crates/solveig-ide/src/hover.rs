//! Hover lookup over retained checked artifacts.
//! The IDE layer renders types and provenance from retained driver snapshots.

use super::position::{
    LiteralTarget, TargetKind, position_index_for_target, target_at_offset_in_artifacts,
    target_at_position_in_artifacts,
};
use super::*;
use solveig_ast::ast::PRELUDE_LIST_NIL_NAME;

pub(super) fn hover_in_artifacts(
    artifacts: &CompilationArtifacts,
    file: &Path,
    offset: usize,
) -> Option<HoverInfo> {
    let target = target_at_offset_in_artifacts(artifacts, file, offset)?;
    hover_for_target(artifacts, target)
}

pub(super) fn hover_at_position_in_artifacts(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
) -> Option<HoverInfo> {
    let target = target_at_position_in_artifacts(artifacts, source_map, file, position, encoding)?;
    hover_for_target(artifacts, target)
}

fn hover_for_target(
    artifacts: &CompilationArtifacts,
    target: position::PositionTarget,
) -> Option<HoverInfo> {
    let pretty_tables = PrettyTables::new(
        &artifacts.names,
        &artifacts.tycon_table,
        &artifacts.class_table,
    );
    match target.kind {
        TargetKind::Local => position_index_for_target(artifacts, target)?
            .local_hover(target)
            .cloned(),
        TargetKind::Definition(def) | TargetKind::Reference(def) => {
            def_hover_info(artifacts, pretty_tables, def.span, def.def_id)
        }
        TargetKind::Module(module) => module_hover_info(artifacts, module.span, module.module_id),
        TargetKind::Literal(literal) => {
            literal_hover_info(artifacts, pretty_tables, target, literal)
        }
        TargetKind::Punctuation if delimiter_token(target.token_kind) => {
            let index = position_index_for_target(artifacts, target)?;
            index.expression_hover(target).cloned()
        }
        TargetKind::Punctuation
        | TargetKind::Identifier
        | TargetKind::Keyword
        | TargetKind::Trivia => None,
    }
}

fn delimiter_token(token: solveig_syntax::lexer::Token) -> bool {
    matches!(
        token,
        solveig_syntax::lexer::Token::LParen
            | solveig_syntax::lexer::Token::RParen
            | solveig_syntax::lexer::Token::LBrack
            | solveig_syntax::lexer::Token::RBrack
    )
}

pub(super) fn hole_hover_infos(
    holes: &[HoleInfo],
    constructor_types: &FxHashMap<CtorId, Ty>,
    ctor_table: &CtorTable,
    pretty_tables: PrettyTables<'_>,
) -> Vec<HoverInfo> {
    holes
        .iter()
        .map(|hole| hole_hover_info(hole, constructor_types, ctor_table, pretty_tables))
        .collect()
}

fn hole_hover_info(
    hole: &HoleInfo,
    constructor_types: &FxHashMap<CtorId, Ty>,
    ctor_table: &CtorTable,
    pretty_tables: PrettyTables<'_>,
) -> HoverInfo {
    let expected = source_signature::pretty_source_ty(
        &solveig_check::inference::holes::prettify_evars(&hole.expected_type),
        pretty_tables,
    );
    let mut markdown = format!(
        "**Unfilled hole**\n\nfound hole of type `{expected}`\n\nExpected type: `{expected}`"
    );

    if !hole.context.is_empty() {
        markdown.push_str("\n\nBindings in scope:\n\n```solveig\n");
        for (name, ty) in &hole.context {
            let ty = source_signature::pretty_source_ty(
                &solveig_check::inference::holes::prettify_evars(ty),
                pretty_tables,
            );
            markdown.push_str(name);
            markdown.push_str(" : ");
            markdown.push_str(&ty);
            markdown.push('\n');
        }
        markdown.push_str("```");
    }

    let suggestions = hole.suggestions(constructor_types, ctor_table, pretty_tables.names);
    if !suggestions.is_empty() {
        markdown.push_str("\n\nValid fills include:\n\n");
        for suggestion in suggestions {
            markdown.push_str("- `");
            markdown.push_str(&suggestion);
            markdown.push_str("`\n");
        }
    }

    HoverInfo::new(
        hole.span.file,
        hole.span.span.to_usize_range(),
        format!("found hole of type `{expected}`"),
        None,
    )
    .with_markdown(markdown)
}

fn literal_hover_info(
    artifacts: &CompilationArtifacts,
    pretty_tables: PrettyTables<'_>,
    target: position::PositionTarget,
    literal: LiteralTarget,
) -> Option<HoverInfo> {
    let signature = match literal {
        LiteralTarget::Int => Some("Int".to_string()),
        LiteralTarget::Float => Some("Float".to_string()),
        LiteralTarget::Char => Some("Char".to_string()),
        LiteralTarget::String => Some("String".to_string()),
        LiteralTarget::EmptyList => {
            return constructor_hover_info_by_full_name(
                artifacts,
                pretty_tables,
                target.file,
                target.token_span.span.to_usize_range(),
                PRELUDE_LIST_NIL_NAME,
            );
        }
    }?;

    Some(HoverInfo::new(
        target.file,
        target.token_span.span.to_usize_range(),
        signature,
        None,
    ))
}

fn constructor_hover_info_by_full_name(
    artifacts: &CompilationArtifacts,
    pretty_tables: PrettyTables<'_>,
    file_id: FileId,
    span: Range<usize>,
    full_name: &str,
) -> Option<HoverInfo> {
    let info = artifacts.ctor_table.iter().find_map(|(_, info)| {
        (artifacts.names.symbol_display_name(info.name) == full_name).then_some(info)
    })?;
    let signature = format!(
        "{} : {}",
        artifacts.names.symbol_display_name(info.short_name),
        source_signature::pretty_source_ty(&info.full_type, pretty_tables)
    );
    let provenance = module_provenance_for_qualified_name(artifacts, full_name);

    Some(HoverInfo::new(file_id, span, signature, None).with_optional_provenance(provenance))
}

fn module_provenance_for_qualified_name(
    artifacts: &CompilationArtifacts,
    full_name: &str,
) -> Option<String> {
    let (module_name, _) = full_name.rsplit_once('.')?;
    let module_id = artifacts
        .module_display_names
        .iter()
        .find_map(|(module_id, display)| (display == module_name).then_some(*module_id))?;

    module_provenance(artifacts, module_id)
}

fn module_hover_info(
    artifacts: &CompilationArtifacts,
    span: FileSpan,
    module_id: ModuleId,
) -> Option<HoverInfo> {
    let name = artifacts.module_display_names.get(&module_id)?;
    let documentation = artifacts
        .module_paths
        .get(&module_id)
        .map(|path| path.display().to_string());

    let provenance = module_provenance(artifacts, module_id);

    Some(
        HoverInfo::new(
            span.file,
            span.span.to_usize_range(),
            format!("module {name}"),
            documentation,
        )
        .with_optional_provenance(provenance),
    )
}

fn def_hover_info(
    artifacts: &CompilationArtifacts,
    pretty_tables: PrettyTables<'_>,
    span: FileSpan,
    def_id: DefId,
) -> Option<HoverInfo> {
    let signature = def_signature(artifacts, pretty_tables, def_id)?;
    let documentation = artifacts.docs.get(&def_id).map(|doc| doc.text.clone());

    Some(
        HoverInfo::new(
            span.file,
            span.span.to_usize_range(),
            signature,
            documentation,
        )
        .with_optional_provenance(def_provenance(artifacts, def_id)),
    )
}

fn def_signature(
    artifacts: &CompilationArtifacts,
    pretty_tables: PrettyTables<'_>,
    def_id: DefId,
) -> Option<String> {
    class_method_signature(artifacts, pretty_tables, def_id)
        .or_else(|| term_signature(artifacts, pretty_tables, def_id))
        .or_else(|| constructor_signature(artifacts, pretty_tables, def_id))
        .or_else(|| tycon_signature(artifacts, def_id))
        .or_else(|| class_signature(artifacts, pretty_tables, def_id))
}

fn term_signature(
    artifacts: &CompilationArtifacts,
    pretty_tables: PrettyTables<'_>,
    def_id: DefId,
) -> Option<String> {
    if let Some(signature) = artifacts.source_signatures.get(&def_id) {
        return Some(signature.clone());
    }

    let scheme = artifacts
        .checked
        .values()
        .find_map(|checked| checked.func_schemes.get(&def_id))?;

    Some(format!(
        "{} : {}",
        short_def_name(&artifacts.names, def_id),
        source_signature::pretty_source_scheme(scheme, pretty_tables)
    ))
}

fn constructor_signature(
    artifacts: &CompilationArtifacts,
    pretty_tables: PrettyTables<'_>,
    def_id: DefId,
) -> Option<String> {
    let full_name = artifacts.names.def_display_name(def_id);
    let info = artifacts.ctor_table.iter().find_map(|(_, info)| {
        (artifacts.names.symbol_display_name(info.name) == full_name).then_some(info)
    })?;

    Some(format!(
        "{} : {}",
        artifacts.names.symbol_display_name(info.short_name),
        source_signature::pretty_source_ty(&info.full_type, pretty_tables)
    ))
}

fn tycon_signature(artifacts: &CompilationArtifacts, def_id: DefId) -> Option<String> {
    let full_name = artifacts.names.def_display_name(def_id);
    let info = artifacts.tycon_table.iter().find_map(|(_, info)| {
        (artifacts.names.symbol_display_name(info.name) == full_name).then_some(info)
    })?;

    Some(format!(
        "type {} : {}",
        artifacts.names.symbol_display_name(info.short_name),
        info.kind.pretty()
    ))
}

fn class_signature(
    artifacts: &CompilationArtifacts,
    pretty_tables: PrettyTables<'_>,
    def_id: DefId,
) -> Option<String> {
    let full_name = artifacts.names.def_display_name(def_id);
    let info = artifacts.class_table.iter().find_map(|(_, info)| {
        (artifacts.names.symbol_display_name(info.name) == full_name).then_some(info)
    })?;

    Some(format!(
        "class {}",
        class_header(&info.definition, pretty_tables)
    ))
}

fn class_method_signature(
    artifacts: &CompilationArtifacts,
    pretty_tables: PrettyTables<'_>,
    def_id: DefId,
) -> Option<String> {
    for checked in artifacts.checked.values() {
        for class_def in &checked.class_defs {
            if let Some(method) = class_def.methods.iter().find(|method| method.id == def_id) {
                let method_signature = artifacts
                    .source_signatures
                    .get(&def_id)
                    .cloned()
                    .unwrap_or_else(|| {
                        format!(
                            "{} : {}",
                            method.short_name,
                            source_signature::pretty_source_scheme(&method.scheme, pretty_tables)
                        )
                    });
                return Some(format!(
                    "class {}\n{}",
                    class_header(class_def, pretty_tables),
                    method_signature
                ));
            }
        }
    }

    None
}

fn def_provenance(artifacts: &CompilationArtifacts, def_id: DefId) -> Option<String> {
    module_provenance(artifacts, def_id.module)
}

fn module_provenance(artifacts: &CompilationArtifacts, module_id: ModuleId) -> Option<String> {
    let module = artifacts.module_display_names.get(&module_id)?;
    let Some(path) = artifacts.module_paths.get(&module_id) else {
        return Some(format!("Defined in `{module}`."));
    };

    Some(format!("Defined in `{module}` at `{}`.", path.display()))
}

fn class_header(class_def: &ClassDefinition, pretty_tables: PrettyTables<'_>) -> String {
    let mut head = class_def.short_name.clone();
    for (param, _) in &class_def.params {
        head.push(' ');
        head.push_str(&pretty_tables.names.tyvar_display_name(*param));
    }

    if class_def.superclasses.is_empty() {
        head
    } else {
        let superclasses = class_def
            .superclasses
            .iter()
            .map(|pred| pred.pretty_with_tables(pretty_tables))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{superclasses} => {head}")
    }
}

fn short_def_name(names: &Names, def_id: DefId) -> String {
    let display = names.def_display_name(def_id);

    display
        .rsplit('.')
        .next()
        .map_or(display.clone(), |name| name.to_string())
}
