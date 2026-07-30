//! Semantic-token extraction for editor integrations.
//! The driver owns Solveig-aware token classification; protocol crates only encode it.

use super::source_lookup::{is_atomic_source_span, source_file_id_in_source_map};
use super::*;
use solveig_syntax::lexer::{PosToken, Token};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TokenPriority {
    Syntax,
    FallbackIdent,
    Binder,
    ResolvedReference,
    Declaration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TokenCandidate {
    token: SourceSemanticToken,
    priority: TokenPriority,
}

pub(super) fn syntax_semantic_tokens(project: &Project) -> Vec<SourceSemanticToken> {
    let mut out = Vec::new();

    for module_file in project.modules.values() {
        out.extend(import_namespace_tokens(
            module_file.parsed.file,
            module_file.parsed.cst.tokens(),
        ));
        for token in module_file.parsed.cst.tokens() {
            if let Some(classified) = syntax_semantic_token(module_file.parsed.file, token) {
                out.push(classified);
            }
        }
        push_local_binders(&module_file.parsed.defs, module_file.parsed.file, &mut out);
    }

    sort_and_dedup_tokens(&mut out);
    out
}

pub(super) fn semantic_tokens_in_compilation(
    artifacts: Option<&CompilationArtifacts>,
    source_map: &SourceMap,
    syntax_tokens: &[SourceSemanticToken],
    file: &Path,
) -> Vec<SourceSemanticToken> {
    let Some(file_id) = source_file_id_in_source_map(source_map, file) else {
        return Vec::new();
    };

    let mut candidates = syntax_tokens
        .iter()
        .copied()
        .filter(|token| token.file_span().file == file_id)
        .map(|token| TokenCandidate {
            token,
            priority: syntax_priority(token.kind()),
        })
        .collect::<Vec<_>>();

    if let Some(artifacts) = artifacts {
        candidates.extend(semantic_candidates(artifacts, file_id));
    }

    select_non_overlapping_tokens(candidates)
}

fn syntax_semantic_token(file: FileId, token: &PosToken) -> Option<SourceSemanticToken> {
    if token.is_synthetic() || token.start() == token.end() {
        return None;
    }

    let kind = syntax_token_kind(&token.kind)?;
    Some(SourceSemanticToken::new(
        FileSpan::new(file, Span::from_usize_range(token.start()..token.end())),
        kind,
        SourceSemanticTokenModifiers::empty(),
    ))
}

fn import_namespace_tokens(file: FileId, tokens: &[PosToken]) -> Vec<SourceSemanticToken> {
    let mut out = Vec::new();
    let mut after_import = false;
    let mut namespace_start = None;
    let mut namespace_end = 0usize;

    for token in tokens {
        if token.is_synthetic() {
            continue;
        }

        if matches!(token.kind, Token::Import) {
            finish_import_namespace(file, namespace_start.take(), namespace_end, &mut out);
            after_import = true;
            namespace_end = 0;
            continue;
        }

        if !after_import {
            continue;
        }

        match &token.kind {
            Token::Whitespace | Token::BlockComment | Token::LineComment => {}
            Token::Ident | Token::UpperIdent | Token::Dot => {
                namespace_start.get_or_insert(token.start());
                namespace_end = token.end();
            }
            _ => {
                finish_import_namespace(file, namespace_start.take(), namespace_end, &mut out);
                after_import = false;
            }
        }
    }

    finish_import_namespace(file, namespace_start, namespace_end, &mut out);
    out
}

fn finish_import_namespace(
    file: FileId,
    start: Option<usize>,
    end: usize,
    out: &mut Vec<SourceSemanticToken>,
) {
    let Some(start) = start else {
        return;
    };
    if start >= end {
        return;
    }

    out.push(SourceSemanticToken::new(
        FileSpan::new(file, Span::from_usize_range(start..end)),
        SourceSemanticTokenKind::Namespace,
        SourceSemanticTokenModifiers::empty(),
    ));
}

fn syntax_token_kind(kind: &Token) -> Option<SourceSemanticTokenKind> {
    match kind {
        Token::LineComment | Token::BlockComment => Some(SourceSemanticTokenKind::Comment),
        Token::Str | Token::Char => Some(SourceSemanticTokenKind::String),
        Token::Int | Token::Float => Some(SourceSemanticTokenKind::Number),
        Token::Let
        | Token::In
        | Token::Do
        | Token::Fun
        | Token::Forall
        | Token::Case
        | Token::Of
        | Token::If
        | Token::Then
        | Token::Else
        | Token::Data
        | Token::Type
        | Token::Sig
        | Token::Class
        | Token::Instance
        | Token::Where
        | Token::Default
        | Token::Import
        | Token::As
        | Token::Infixl
        | Token::Infixr
        | Token::Infix => Some(SourceSemanticTokenKind::Keyword),
        Token::Eq
        | Token::Colon
        | Token::Arrow
        | Token::BindArrow
        | Token::FatArrow
        | Token::Cons
        | Token::Pipe
        | Token::Dot
        | Token::Plus
        | Token::Minus
        | Token::Star
        | Token::Slash
        | Token::Percent
        | Token::EqEq
        | Token::Less
        | Token::Greater
        | Token::LessEq
        | Token::GreaterEq
        | Token::Backtick
        | Token::Operator
        | Token::ReservedOp => Some(SourceSemanticTokenKind::Operator),
        Token::Ident => Some(SourceSemanticTokenKind::Variable),
        Token::UpperIdent => Some(SourceSemanticTokenKind::Type),
        Token::Whitespace
        | Token::Newline
        | Token::Hole
        | Token::Underscore
        | Token::Semicolon
        | Token::Comma
        | Token::LParen
        | Token::RParen
        | Token::LBrack
        | Token::RBrack
        | Token::VOpen
        | Token::VClose
        | Token::VSemi
        | Token::Eof => None,
    }
}

fn semantic_candidates(artifacts: &CompilationArtifacts, file_id: FileId) -> Vec<TokenCandidate> {
    let mut out = Vec::new();
    let def_kinds = def_kinds(artifacts);

    for symbol in &artifacts.source_symbols {
        push_symbol_declarations(symbol, file_id, &mut out);
    }

    for (span, def_id) in &artifacts.declaration_spans {
        if span.file != file_id || span.is_empty() {
            continue;
        }

        let Some(kind) = def_kinds.get(def_id).copied() else {
            continue;
        };
        out.push(TokenCandidate {
            token: SourceSemanticToken::new(*span, kind, declaration_modifiers(kind)),
            priority: TokenPriority::Declaration,
        });
    }

    let source_file = artifacts.source_files.get(file_id.0 as usize);
    for resolutions in artifacts.resolutions.values() {
        for (span, def_id) in resolutions {
            if span.file != file_id || span.is_empty() {
                continue;
            }
            let Some(source_file) = source_file else {
                continue;
            };
            if !is_atomic_source_span(source_file, span.span) {
                continue;
            }

            let Some(mut kind) = def_kinds.get(def_id).copied() else {
                continue;
            };
            if is_operator_source_span(source_file, span.span) {
                kind = SourceSemanticTokenKind::Operator;
            }
            let mut modifiers =
                SourceSemanticTokenModifiers::one(SourceSemanticTokenModifier::Readonly);
            if is_default_library_def(artifacts, def_id) {
                modifiers = modifiers.with(SourceSemanticTokenModifier::DefaultLibrary);
            }

            out.push(TokenCandidate {
                token: SourceSemanticToken::new(*span, kind, modifiers),
                priority: TokenPriority::ResolvedReference,
            });
        }
    }

    out
}

fn push_symbol_declarations(symbol: &SourceSymbol, file_id: FileId, out: &mut Vec<TokenCandidate>) {
    let kind = kind_for_source_symbol(symbol.kind());
    if symbol.selection_span().file == file_id {
        out.push(TokenCandidate {
            token: SourceSemanticToken::new(
                symbol.selection_span(),
                kind,
                declaration_modifiers(kind),
            ),
            priority: TokenPriority::Declaration,
        });
    }

    for child in symbol.children() {
        push_symbol_declarations(child, file_id, out);
    }
}

fn push_local_binders(
    definitions: &[Spanned<SurfaceDefinition>],
    file: FileId,
    out: &mut Vec<SourceSemanticToken>,
) {
    for definition in definitions {
        match &definition.0 {
            SurfaceDefinition::FunDef { params, body, .. } => {
                push_value_params(params, file, out);
                push_expr_binders(body, file, out);
            }
            SurfaceDefinition::SigDef { ty, .. } => push_type_binders(ty, file, out),
            SurfaceDefinition::FixityDef { .. } => {}
            SurfaceDefinition::DataDef {
                ty_params,
                constructors,
                ..
            } => {
                push_type_params(ty_params, file, out);
                for constructor in constructors {
                    for param in &constructor.0.params {
                        push_type_binders(param, file, out);
                    }
                }
            }
            SurfaceDefinition::TypeAliasDef { ty_params, ty, .. } => {
                push_type_params(ty_params, file, out);
                push_type_binders(ty, file, out);
            }
            SurfaceDefinition::ClassDef {
                ty_params,
                superclasses,
                methods,
                defaults,
                ..
            } => {
                push_type_params(ty_params, file, out);
                for superclass in superclasses {
                    push_predicate_type_binders(superclass, file, out);
                }
                for method in methods {
                    push_type_binders(&method.ty, file, out);
                }
                for default in defaults {
                    push_value_params(&default.params, file, out);
                    push_expr_binders(&default.body, file, out);
                }
            }
            SurfaceDefinition::InstanceDef {
                context,
                args,
                methods,
                ..
            } => {
                for predicate in context {
                    push_predicate_type_binders(predicate, file, out);
                }
                for arg in args {
                    push_type_binders(arg, file, out);
                }
                for method in methods {
                    push_value_params(&method.params, file, out);
                    push_expr_binders(&method.body, file, out);
                }
            }
        }
    }
}

fn push_expr_binders(
    expr: &Spanned<SurfaceExpr>,
    file: FileId,
    out: &mut Vec<SourceSemanticToken>,
) {
    match &expr.0 {
        SurfaceExpr::Unit
        | SurfaceExpr::Literal(_)
        | SurfaceExpr::Hole
        | SurfaceExpr::Var(_)
        | SurfaceExpr::Con(_) => {}
        SurfaceExpr::App(lhs, rhs) | SurfaceExpr::OperatorApply(lhs, _, rhs) => {
            push_expr_binders(lhs, file, out);
            push_expr_binders(rhs, file, out);
        }
        SurfaceExpr::Abs(params, body) => {
            for (param, annotation) in params {
                push_param(*param, file, out);
                if let Some(ty) = annotation {
                    push_type_binders(ty, file, out);
                }
            }
            push_expr_binders(body, file, out);
        }
        SurfaceExpr::Ann(value, ty) => {
            push_expr_binders(value, file, out);
            push_type_binders(ty, file, out);
        }
        SurfaceExpr::Let(binder, value, body) => {
            push_param(*binder, file, out);
            push_expr_binders(value, file, out);
            push_expr_binders(body, file, out);
        }
        SurfaceExpr::Case(scrutinee, arms) => {
            push_expr_binders(scrutinee, file, out);
            for arm in arms {
                push_pattern_binders(&arm.0.pattern, file, out);
                push_expr_binders(&arm.0.expr, file, out);
            }
        }
        SurfaceExpr::Do(statements) => {
            for statement in statements {
                match &statement.0 {
                    SurfaceDoStmt::Bind { binder, expr } | SurfaceDoStmt::Let { binder, expr } => {
                        push_param(*binder, file, out);
                        push_expr_binders(expr, file, out);
                    }
                    SurfaceDoStmt::Expr(expr) => push_expr_binders(expr, file, out),
                }
            }
        }
        SurfaceExpr::IfThenElse(cond, then_expr, else_expr) => {
            push_expr_binders(cond, file, out);
            push_expr_binders(then_expr, file, out);
            push_expr_binders(else_expr, file, out);
        }
        SurfaceExpr::UnaryOperator(_, value)
        | SurfaceExpr::OperatorLeftSection(value, _)
        | SurfaceExpr::OperatorRightSection(_, value) => push_expr_binders(value, file, out),
        SurfaceExpr::OpSeq(first, rest) => {
            push_expr_binders(&first.expr, file, out);
            for segment in rest {
                push_expr_binders(&segment.rhs.expr, file, out);
            }
        }
    }
}

fn push_pattern_binders(
    pattern: &Spanned<SurfacePattern>,
    file: FileId,
    out: &mut Vec<SourceSemanticToken>,
) {
    match &pattern.0 {
        SurfacePattern::Var(symbol) => push_param((*symbol, pattern.1, pattern.2), file, out),
        SurfacePattern::Lit(_) | SurfacePattern::Wild => {}
        SurfacePattern::Con(_, args) => {
            for arg in args {
                push_pattern_binders(arg, file, out);
            }
        }
    }
}

fn push_type_binders(ty: &Spanned<SurfaceTy>, file: FileId, out: &mut Vec<SourceSemanticToken>) {
    match &ty.0 {
        SurfaceTy::Unit | SurfaceTy::Con(_) => {}
        SurfaceTy::Var(_) => push_type_param_reference(ty.1, file, out),
        SurfaceTy::Arr(from, to) | SurfaceTy::App(from, to) => {
            push_type_binders(from, file, out);
            push_type_binders(to, file, out);
        }
        SurfaceTy::ForAll(params, body) => {
            push_type_params(params, file, out);
            push_type_binders(body, file, out);
        }
        SurfaceTy::Qualified(predicates, body) => {
            for predicate in predicates {
                push_predicate_type_binders(predicate, file, out);
            }
            push_type_binders(body, file, out);
        }
    }
}

fn push_type_param_reference(span: Span, file: FileId, out: &mut Vec<SourceSemanticToken>) {
    if span.is_empty() {
        return;
    }

    out.push(SourceSemanticToken::new(
        FileSpan::new(file, span),
        SourceSemanticTokenKind::TypeParameter,
        SourceSemanticTokenModifiers::empty(),
    ));
}

fn push_predicate_type_binders(
    predicate: &Spanned<SurfacePredicate>,
    file: FileId,
    out: &mut Vec<SourceSemanticToken>,
) {
    for arg in &predicate.0.args {
        push_type_binders(arg, file, out);
    }
}

fn push_value_params(params: &[Spanned<Symbol>], file: FileId, out: &mut Vec<SourceSemanticToken>) {
    for param in params {
        push_param(*param, file, out);
    }
}

fn push_type_params(params: &[Spanned<Symbol>], file: FileId, out: &mut Vec<SourceSemanticToken>) {
    for param in params {
        if !param.1.is_empty() {
            out.push(SourceSemanticToken::new(
                FileSpan::new(file, param.1),
                SourceSemanticTokenKind::TypeParameter,
                SourceSemanticTokenModifiers::one(SourceSemanticTokenModifier::Declaration),
            ));
        }
    }
}

fn push_param(param: Spanned<Symbol>, file: FileId, out: &mut Vec<SourceSemanticToken>) {
    if param.1.is_empty() {
        return;
    }

    out.push(SourceSemanticToken::new(
        FileSpan::new(file, param.1),
        SourceSemanticTokenKind::Parameter,
        SourceSemanticTokenModifiers::one(SourceSemanticTokenModifier::Declaration)
            .with(SourceSemanticTokenModifier::Readonly),
    ));
}

fn def_kinds(artifacts: &CompilationArtifacts) -> FxHashMap<DefId, SourceSemanticTokenKind> {
    let mut out = FxHashMap::default();

    for symbol in &artifacts.source_symbols {
        collect_symbol_def_kinds(artifacts, symbol, &mut out);
    }

    out
}

fn collect_symbol_def_kinds(
    artifacts: &CompilationArtifacts,
    symbol: &SourceSymbol,
    out: &mut FxHashMap<DefId, SourceSemanticTokenKind>,
) {
    let Some(def_id) = def_id_for_span(artifacts, symbol.selection_span()) else {
        return;
    };

    out.insert(def_id, kind_for_source_symbol(symbol.kind()));
    for child in symbol.children() {
        collect_symbol_def_kinds(artifacts, child, out);
    }
}

fn def_id_for_span(artifacts: &CompilationArtifacts, span: FileSpan) -> Option<DefId> {
    artifacts
        .declaration_spans
        .iter()
        .find_map(|(decl_span, def_id)| (*decl_span == span).then_some(*def_id))
}

fn kind_for_source_symbol(kind: SourceSymbolKind) -> SourceSemanticTokenKind {
    match kind {
        SourceSymbolKind::Function => SourceSemanticTokenKind::Function,
        SourceSymbolKind::Type | SourceSymbolKind::TypeAlias => SourceSemanticTokenKind::Type,
        SourceSymbolKind::Constructor => SourceSemanticTokenKind::EnumMember,
        SourceSymbolKind::Class => SourceSemanticTokenKind::Class,
        SourceSymbolKind::Method => SourceSemanticTokenKind::Method,
    }
}

fn declaration_modifiers(kind: SourceSemanticTokenKind) -> SourceSemanticTokenModifiers {
    let mut modifiers = SourceSemanticTokenModifiers::one(SourceSemanticTokenModifier::Declaration)
        .with(SourceSemanticTokenModifier::Definition);

    if matches!(
        kind,
        SourceSemanticTokenKind::Function
            | SourceSemanticTokenKind::EnumMember
            | SourceSemanticTokenKind::Method
            | SourceSemanticTokenKind::Parameter
            | SourceSemanticTokenKind::Variable
    ) {
        modifiers = modifiers.with(SourceSemanticTokenModifier::Readonly);
    }

    modifiers
}

fn is_default_library_def(artifacts: &CompilationArtifacts, def_id: &DefId) -> bool {
    artifacts
        .module_paths
        .get(&def_id.module)
        .is_some_and(|path| path.starts_with("<compiler-prelude>"))
}

fn is_operator_source_span(source_file: &SourceFile, span: Span) -> bool {
    let range = span.to_usize_range();
    let Some(text) = source_file.src.get(range) else {
        return false;
    };

    !text.is_empty()
        && !text
            .chars()
            .any(|ch| ch == '_' || ch == '\'' || ch.is_ascii_alphanumeric())
}

fn select_non_overlapping_tokens(mut candidates: Vec<TokenCandidate>) -> Vec<SourceSemanticToken> {
    candidates.retain(|candidate| {
        let span = candidate.token.file_span();
        span.span.start < span.span.end
    });
    candidates.sort_by(|left, right| {
        let left_span = left.token.file_span();
        let right_span = right.token.file_span();

        (left_span.file, left_span.span.start)
            .cmp(&(right_span.file, right_span.span.start))
            .then_with(|| right.priority.cmp(&left.priority))
            .then_with(|| right_span.span.end.cmp(&left_span.span.end))
            .then_with(|| left.token.kind().cmp(&right.token.kind()))
            .then_with(|| left.token.modifiers().cmp(&right.token.modifiers()))
    });

    let mut out = Vec::new();
    for candidate in candidates {
        let span = candidate.token.file_span();
        let overlaps_previous = out.last().is_some_and(|previous: &SourceSemanticToken| {
            let previous_span = previous.file_span();
            previous_span.file == span.file && previous_span.span.end > span.span.start
        });
        if !overlaps_previous {
            out.push(candidate.token);
        }
    }

    out
}

fn sort_and_dedup_tokens(tokens: &mut Vec<SourceSemanticToken>) {
    tokens.sort_by_key(token_sort_key);
    tokens.dedup_by_key(|token| {
        let span = token.file_span();
        (
            span.file,
            span.span.start,
            span.span.end,
            token.kind(),
            token.modifiers(),
        )
    });
}

fn token_sort_key(token: &SourceSemanticToken) -> (FileId, u32, u32, SourceSemanticTokenKind) {
    let span = token.file_span();

    (span.file, span.span.start, span.span.end, token.kind())
}

fn syntax_priority(kind: SourceSemanticTokenKind) -> TokenPriority {
    match kind {
        SourceSemanticTokenKind::Variable | SourceSemanticTokenKind::Type => {
            TokenPriority::FallbackIdent
        }
        SourceSemanticTokenKind::Namespace
        | SourceSemanticTokenKind::TypeParameter
        | SourceSemanticTokenKind::Parameter => TokenPriority::Binder,
        SourceSemanticTokenKind::Class
        | SourceSemanticTokenKind::Function
        | SourceSemanticTokenKind::EnumMember
        | SourceSemanticTokenKind::Method
        | SourceSemanticTokenKind::Keyword
        | SourceSemanticTokenKind::Operator
        | SourceSemanticTokenKind::String
        | SourceSemanticTokenKind::Number
        | SourceSemanticTokenKind::Comment => TokenPriority::Syntax,
    }
}
