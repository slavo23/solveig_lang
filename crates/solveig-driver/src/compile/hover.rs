//! Hover lookup over retained checked artifacts.
//! The driver keeps enough table state here to render types after checking.

use super::source_lookup::{is_atomic_source_span, source_file_id, span_contains_offset, span_len};
use super::*;
use solveig_ast::ast::PRELUDE_LIST_NIL_NAME;
use solveig_syntax::lexer::{Token, lex};

#[derive(Clone, Debug)]
struct HoverCandidate {
    priority: u8,
    info: HoverInfo,
}

#[derive(Clone, Debug)]
struct HoverToken {
    kind: HoverTokenKind,
    span: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HoverTokenKind {
    Semantic,
    ReferenceOnly,
    Delimiter,
    IntLiteral,
    FloatLiteral,
    CharLiteral,
    StringLiteral,
    EmptyList,
    Quiet,
}

impl HoverToken {
    fn allows_semantic_hover(&self) -> bool {
        !matches!(self.kind, HoverTokenKind::Quiet)
    }

    fn allows_expression_fallback(&self) -> bool {
        matches!(
            self.kind,
            HoverTokenKind::Semantic
                | HoverTokenKind::IntLiteral
                | HoverTokenKind::FloatLiteral
                | HoverTokenKind::CharLiteral
                | HoverTokenKind::StringLiteral
        )
    }
}

pub(super) fn hover_in_artifacts(
    artifacts: &CompilationArtifacts,
    file: &Path,
    offset: usize,
) -> Option<HoverInfo> {
    let file_id = source_file_id(artifacts, file)?;
    hover_in_file_id(artifacts, file_id, offset)
}

pub(super) fn hover_at_position_in_artifacts(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
) -> Option<HoverInfo> {
    let file_id = source_file_id(artifacts, file)?;
    let offset = source_map.offset(file_id, position, encoding) as usize;

    hover_in_file_id(artifacts, file_id, offset)
}

fn hover_in_file_id(
    artifacts: &CompilationArtifacts,
    file_id: FileId,
    offset: usize,
) -> Option<HoverInfo> {
    let pretty_tables = PrettyTables {
        names: &artifacts.names,
        tycons: &artifacts.tycon_table,
        classes: &artifacts.class_table,
    };
    let mut best = None;
    let source_file = artifacts.source_files.get(file_id.0 as usize);
    let hover_token = source_file.and_then(|source_file| hover_token_at(source_file, offset));
    let token_allows_hover = hover_token
        .as_ref()
        .is_none_or(HoverToken::allows_semantic_hover);
    let token_allows_expression_fallback = hover_token
        .as_ref()
        .is_none_or(HoverToken::allows_expression_fallback);

    if let Some(info) = hover_token
        .as_ref()
        .and_then(|token| token_hover_info(artifacts, pretty_tables, file_id, token))
    {
        consider_hover_candidate(&mut best, 5, info);
    }

    for info in &artifacts.local_hovers {
        if info.file_span().file != file_id
            || !token_allows_hover
            || !span_contains_offset(&info.span(), offset)
        {
            continue;
        }

        consider_hover_candidate(&mut best, 4, info.clone());
    }

    for (span, module_id) in &artifacts.module_imports {
        if span.file != file_id || !token_allows_hover {
            continue;
        }
        let hover_span = source_file
            .and_then(|source_file| {
                let module_name = artifacts.module_display_names.get(module_id)?;
                source_lookup::import_module_name_span(&source_file.src, span.span, module_name)
            })
            .unwrap_or(span.span);
        if !span_contains_offset(&hover_span.to_usize_range(), offset) {
            continue;
        }

        if let Some(info) =
            module_hover_info(artifacts, FileSpan::new(span.file, hover_span), *module_id)
        {
            consider_hover_candidate(&mut best, 2, info);
        }
    }

    for (span, def_id) in &artifacts.declaration_spans {
        if span.file != file_id || !span_contains_offset(&span.span.to_usize_range(), offset) {
            continue;
        }

        if let Some(info) = def_hover_info(artifacts, pretty_tables, *span, *def_id) {
            consider_hover_candidate(&mut best, 3, info);
        }
    }

    for resolutions in artifacts.resolutions.values() {
        for (span, def_id) in resolutions {
            if span.file != file_id
                || !token_allows_hover
                || !source_file
                    .is_some_and(|source_file| is_atomic_source_span(source_file, span.span))
                || !span_contains_offset(&span.span.to_usize_range(), offset)
            {
                continue;
            }

            if let Some(info) = def_hover_info(artifacts, pretty_tables, *span, *def_id) {
                consider_hover_candidate(&mut best, 3, info);
            }
        }
    }

    if token_allows_expression_fallback {
        for (module_id, checked) in &artifacts.checked {
            if artifacts.module_files.get(module_id) != Some(&file_id) {
                continue;
            }

            for func_def in &checked.func_defs {
                hover_expr(file_id, &func_def.body, pretty_tables, offset, &mut best);
            }

            for instance in &checked.instance_defs {
                for method in &instance.methods {
                    hover_expr(file_id, &method.body, pretty_tables, offset, &mut best);
                }
            }
        }
    }

    best.map(|candidate| candidate.info)
}

fn hover_token_at(source_file: &SourceFile, offset: usize) -> Option<HoverToken> {
    let session = Session::new();
    let file = session.add_file(source_file.path.clone(), source_file.src.clone());
    let tokens = lex(&session, file, &source_file.src).ok()?;

    tokens
        .iter()
        .enumerate()
        .find(|(_, token)| {
            !token.is_synthetic()
                && !token.kind.is_trivia()
                && span_contains_offset(&(token.start..token.end), offset)
        })
        .map(|(index, _)| HoverToken {
            kind: hover_token_kind(&tokens, index),
            span: hover_token_span(&tokens, index),
        })
}

fn hover_token_kind(tokens: &[solveig_syntax::lexer::PosToken], index: usize) -> HoverTokenKind {
    let token = &tokens[index];

    match token.kind {
        Token::Int(_) => HoverTokenKind::IntLiteral,
        Token::Float(_) => HoverTokenKind::FloatLiteral,
        Token::Char(_) => HoverTokenKind::CharLiteral,
        Token::Str(_) => HoverTokenKind::StringLiteral,
        Token::LBrack | Token::RBrack if empty_list_token_span(tokens, index).is_some() => {
            HoverTokenKind::EmptyList
        }
        Token::Ident(_)
        | Token::UpperIdent(_)
        | Token::Hole
        | Token::Operator(_)
        | Token::Plus
        | Token::Minus
        | Token::Star
        | Token::Slash
        | Token::Percent
        | Token::EqEq
        | Token::Less
        | Token::Greater
        | Token::LessEq
        | Token::GreaterEq => HoverTokenKind::Semantic,
        Token::Underscore | Token::Cons => HoverTokenKind::ReferenceOnly,
        Token::LParen | Token::RParen | Token::LBrack | Token::RBrack => HoverTokenKind::Delimiter,
        _ => HoverTokenKind::Quiet,
    }
}

fn hover_token_span(tokens: &[solveig_syntax::lexer::PosToken], index: usize) -> Range<usize> {
    empty_list_token_span(tokens, index).unwrap_or_else(|| tokens[index].start..tokens[index].end)
}

fn empty_list_token_span(
    tokens: &[solveig_syntax::lexer::PosToken],
    index: usize,
) -> Option<Range<usize>> {
    match tokens[index].kind {
        Token::LBrack => {
            let close = next_significant_token(tokens, index)?;
            matches!(tokens[close].kind, Token::RBrack)
                .then_some(tokens[index].start..tokens[close].end)
        }
        Token::RBrack => {
            let open = previous_significant_token(tokens, index)?;
            matches!(tokens[open].kind, Token::LBrack)
                .then_some(tokens[open].start..tokens[index].end)
        }
        _ => None,
    }
}

fn next_significant_token(
    tokens: &[solveig_syntax::lexer::PosToken],
    index: usize,
) -> Option<usize> {
    tokens
        .iter()
        .enumerate()
        .skip(index + 1)
        .find(|(_, token)| !token.is_synthetic() && !token.kind.is_trivia())
        .map(|(index, _)| index)
}

fn previous_significant_token(
    tokens: &[solveig_syntax::lexer::PosToken],
    index: usize,
) -> Option<usize> {
    tokens[..index]
        .iter()
        .enumerate()
        .rev()
        .find(|(_, token)| !token.is_synthetic() && !token.kind.is_trivia())
        .map(|(index, _)| index)
}

fn token_hover_info(
    artifacts: &CompilationArtifacts,
    pretty_tables: PrettyTables<'_>,
    file_id: FileId,
    token: &HoverToken,
) -> Option<HoverInfo> {
    let signature = match token.kind {
        HoverTokenKind::IntLiteral => Some("Int".to_string()),
        HoverTokenKind::FloatLiteral => Some("Float".to_string()),
        HoverTokenKind::CharLiteral => Some("Char".to_string()),
        HoverTokenKind::StringLiteral => Some("String".to_string()),
        HoverTokenKind::EmptyList => {
            return constructor_hover_info_by_full_name(
                artifacts,
                pretty_tables,
                file_id,
                token.span.clone(),
                PRELUDE_LIST_NIL_NAME,
            );
        }
        HoverTokenKind::Semantic
        | HoverTokenKind::ReferenceOnly
        | HoverTokenKind::Delimiter
        | HoverTokenKind::Quiet => None,
    }?;

    Some(HoverInfo::new(file_id, token.span.clone(), signature, None))
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

fn hover_expr(
    file_id: FileId,
    expr: &TermExprNode<TcMetadata, Range<usize>>,
    pretty_tables: PrettyTables<'_>,
    offset: usize,
    best: &mut Option<HoverCandidate>,
) {
    if !span_contains_offset(&expr.anno.span, offset) {
        return;
    }

    if let Some(body) = generated_hover_wrapper_body(expr) {
        hover_expr(file_id, body, pretty_tables, offset, best);
        return;
    }

    if !expr.anno.span.is_empty() {
        consider_hover_candidate(
            best,
            1,
            HoverInfo::new(
                file_id,
                expr.anno.span.clone(),
                expr.anno.inferred_type.pretty_with_tables(pretty_tables),
                None,
            ),
        );
    }

    match &expr.node {
        TermExpr::Unit
        | TermExpr::Var(_)
        | TermExpr::Lit(_)
        | TermExpr::Hole
        | TermExpr::Con(_) => {}
        TermExpr::App(func, arg) => {
            hover_expr(file_id, func, pretty_tables, offset, best);
            hover_expr(file_id, arg, pretty_tables, offset, best);
        }
        TermExpr::Abs(_, _, body)
        | TermExpr::TyAbs(_, _, body)
        | TermExpr::TyApp(body, _)
        | TermExpr::EvAbs(_, _, body)
        | TermExpr::EvApp(body, _)
        | TermExpr::Anno(body, _) => hover_expr(file_id, body, pretty_tables, offset, best),
        TermExpr::Case(scrutinee, arms) => {
            hover_expr(file_id, scrutinee, pretty_tables, offset, best);
            for arm in arms {
                hover_expr(file_id, &arm.node.expr, pretty_tables, offset, best);
            }
        }
        TermExpr::IfThenElse(cond, then_branch, else_branch) => {
            hover_expr(file_id, cond, pretty_tables, offset, best);
            hover_expr(file_id, then_branch, pretty_tables, offset, best);
            hover_expr(file_id, else_branch, pretty_tables, offset, best);
        }
        TermExpr::Let(_, binder, in_expr) => {
            hover_expr(file_id, binder, pretty_tables, offset, best);
            hover_expr(file_id, in_expr, pretty_tables, offset, best);
        }
    }
}

fn generated_hover_wrapper_body(
    expr: &TermExprNode<TcMetadata, Range<usize>>,
) -> Option<&TermExprNode<TcMetadata, Range<usize>>> {
    match &expr.node {
        TermExpr::TyAbs(_, _, body)
        | TermExpr::TyApp(body, _)
        | TermExpr::EvAbs(_, _, body)
        | TermExpr::EvApp(body, _) => Some(body),
        _ => None,
    }
}

fn consider_hover_candidate(best: &mut Option<HoverCandidate>, priority: u8, info: HoverInfo) {
    let replace = best.as_ref().is_none_or(|current| {
        let current_len = span_len(&current.info.span());
        let candidate_len = span_len(&info.span());

        candidate_len < current_len
            || (candidate_len == current_len && priority >= current.priority)
    });

    if replace {
        *best = Some(HoverCandidate { priority, info });
    }
}
