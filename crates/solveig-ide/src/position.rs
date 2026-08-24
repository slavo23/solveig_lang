//! Shared source-position resolution for protocol-agnostic IDE features.
//! This module owns the single offset-to-target path required by Phase R:
//! CST token lookup, parent walk, nearest AST-bearing CST node, and snapshot
//! artifact classification.

use super::hover_artifacts::RetainedIdeArtifacts;
use super::source_lookup::{import_module_name_span, source_file_id, source_file_id_in_source_map};
use super::*;
use solveig_syntax::ast::{SurfaceOpOperand, SurfaceOpSegment};
use solveig_syntax::cst::{CstId, SyntaxTree, TokenId};
use solveig_syntax::lexer::Token;

mod instance_equations;

use instance_equations::collect_instance_method_group_position_facts;

type CheckedExpr = TermExprNode<TcMetadata, Range<usize>>;
type CheckedExprRef<'a> = &'a CheckedExpr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DefTarget {
    pub(crate) span: FileSpan,
    pub(crate) def_id: DefId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ModuleTarget {
    pub(crate) span: FileSpan,
    pub(crate) module_id: ModuleId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LiteralTarget {
    Int,
    Float,
    Char,
    String,
    EmptyList,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TargetKind {
    Definition(DefTarget),
    Reference(DefTarget),
    Local,
    Module(ModuleTarget),
    Literal(LiteralTarget),
    Identifier,
    Keyword,
    Punctuation,
    Trivia,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PositionTarget {
    pub(crate) file: FileId,
    pub(crate) offset: usize,
    pub(crate) token: TokenId,
    pub(crate) token_kind: Token,
    pub(crate) token_span: FileSpan,
    pub(crate) node: CstId,
    pub(crate) ast: Option<CstId>,
    pub(crate) ast_span: Option<FileSpan>,
    pub(crate) kind: TargetKind,
}

#[derive(Clone, Debug)]
struct SignatureCallSite {
    source_span: Range<usize>,
    signature: SourceSignatureInformation,
    argument_spans: Vec<Range<usize>>,
}

impl SignatureCallSite {
    fn help_at(&self, offset: usize) -> Option<SourceSignatureHelp> {
        span_contains_offset_inclusive(&self.source_span, offset).then(|| {
            SourceSignatureHelp::new(self.signature.clone(), 0, self.active_parameter(offset))
        })
    }

    fn active_parameter(&self, offset: usize) -> u32 {
        let mut active = 0usize;

        for (index, span) in self.argument_spans.iter().enumerate() {
            active = index;
            if offset <= span.end {
                break;
            }
        }

        u32::try_from(active.min(self.argument_spans.len().saturating_sub(1))).unwrap_or(u32::MAX)
    }
}

#[derive(Clone, Copy)]
struct PositionFactContext<'a> {
    snapshot: &'a CompilationSnapshot,
    docs: &'a FxHashMap<DefId, DocComment>,
    ctor_table: &'a CtorTable,
    tables: PrettyTables<'a>,
}

#[derive(Clone, Debug)]
pub(crate) struct PositionIndex {
    file: FileId,
    cst: SyntaxTree,
    top_level_definitions: Vec<CstId>,
    ast_nodes: Vec<bool>,
    definitions: FxHashMap<Span, DefTarget>,
    references: FxHashMap<Span, DefTarget>,
    modules: FxHashMap<Span, ModuleTarget>,
    local_hovers: FxHashMap<Span, HoverInfo>,
    tuple_pattern_nodes: FxHashMap<Span, CstId>,
    expression_hovers: Vec<Option<HoverInfo>>,
    signature_call_sites: Vec<Option<SignatureCallSite>>,
}

impl PositionIndex {
    fn new(parsed: &ParsedModule) -> Self {
        let node_count = parsed.cst.nodes().count();
        let tuple_pattern_nodes = parsed
            .cst
            .nodes()
            .filter_map(|(id, node)| {
                (node.kind() == SyntaxKind::TuplePattern).then_some((node.span(), id))
            })
            .collect();
        let mut index = Self {
            file: parsed.file,
            cst: parsed.cst.clone(),
            top_level_definitions: parsed.defs.iter().map(|definition| definition.2).collect(),
            ast_nodes: vec![false; node_count],
            definitions: FxHashMap::default(),
            references: FxHashMap::default(),
            modules: FxHashMap::default(),
            local_hovers: FxHashMap::default(),
            tuple_pattern_nodes,
            expression_hovers: vec![None; node_count],
            signature_call_sites: vec![None; node_count],
        };
        index.mark_module_ast_nodes(parsed);
        index
    }

    pub(crate) fn target_at(&self, offset: usize) -> Option<PositionTarget> {
        let token = self.cst.token_at_offset(offset)?;
        let pos_token = self.cst.token(token);
        let node = self.cst.token_parent(token)?;
        let ast = self.nearest_ast_node(node);
        let token_span = FileSpan::new(
            self.file,
            Span::from_usize_range(pos_token.start()..pos_token.end()),
        );
        let ast_span = ast.map(|id| FileSpan::new(self.file, self.cst.node(id).span()));
        let kind = self.classify_token(token, pos_token.kind, token_span.span);

        Some(PositionTarget {
            file: self.file,
            offset,
            token,
            token_kind: pos_token.kind,
            token_span,
            node,
            ast,
            ast_span,
            kind,
        })
    }

    pub(crate) fn syntax_tree(&self) -> &SyntaxTree {
        &self.cst
    }

    pub(crate) fn is_format_safe(&self) -> bool {
        !self
            .cst
            .nodes()
            .any(|(_, node)| node.kind() == SyntaxKind::Error)
    }

    pub(crate) fn top_level_definition_intersecting(&self, range: Span) -> Option<CstId> {
        let mut matches = self
            .top_level_definitions
            .iter()
            .copied()
            .filter(|definition| {
                let definition = self.cst.node(*definition).span();
                if range.is_empty() {
                    definition.start <= range.start && range.start < definition.end
                } else {
                    definition.start < range.end && range.start < definition.end
                }
            });
        let definition = matches.next()?;

        matches.next().is_none().then_some(definition)
    }

    pub(crate) fn local_hover(&self, target: PositionTarget) -> Option<&HoverInfo> {
        self.local_hovers.get(&target.token_span.span)
    }

    pub(crate) fn expression_hover(&self, target: PositionTarget) -> Option<&HoverInfo> {
        let ast = target.ast?;
        self.expression_hovers
            .get(ast.index())
            .and_then(Option::as_ref)
    }

    pub(crate) fn signature_help(&self, target: PositionTarget) -> Option<SourceSignatureHelp> {
        let mut current = Some(target.node);
        while let Some(id) = current {
            if let Some(call_site) = self
                .signature_call_sites
                .get(id.index())
                .and_then(Option::as_ref)
                && let Some(help) = call_site.help_at(target.offset)
            {
                return Some(help);
            }
            current = self.cst.parent(id);
        }

        None
    }

    pub(crate) fn selection_range_at(&self, offset: usize) -> Option<SourceSelectionRange> {
        let target = self.target_at(offset)?;
        let mut spans = Vec::new();

        if !target.token_span.span.is_empty() {
            spans.push(target.token_span);
        }
        self.push_parent_node_spans(Some(target.node), &mut spans);
        spans.dedup();

        let mut parent = None;
        for span in spans.into_iter().rev() {
            parent = Some(SourceSelectionRange::new(span, parent));
        }

        parent
    }

    fn mark_module_ast_nodes(&mut self, parsed: &ParsedModule) {
        for import in &parsed.imports {
            self.mark_ast_node(import.cst_id);
        }
        for definition in &parsed.defs {
            self.mark_definition_ast_nodes(definition);
        }
    }

    fn mark_definition_ast_nodes(&mut self, definition: &Spanned<SurfaceDefinition>) {
        self.mark_ast_node(definition.2);
        match &definition.0 {
            SurfaceDefinition::FunDef { params, body, .. } => {
                for param in params {
                    self.mark_pattern_ast_nodes(param);
                }
                self.mark_expr_ast_nodes(body);
            }
            SurfaceDefinition::SigDef { ty, .. } | SurfaceDefinition::TypeAliasDef { ty, .. } => {
                self.mark_type_ast_nodes(ty);
            }
            SurfaceDefinition::FixityDef { .. } => {}
            SurfaceDefinition::DataDef {
                constructors,
                ty_params: _,
                ..
            } => {
                for constructor in constructors {
                    self.mark_ast_node(constructor.2);
                    for param in &constructor.0.params {
                        self.mark_type_ast_nodes(param);
                    }
                }
            }
            SurfaceDefinition::ClassDef {
                superclasses,
                methods,
                defaults,
                ..
            } => {
                for superclass in superclasses {
                    self.mark_ast_node(superclass.2);
                    self.mark_type_ast_nodes(&superclass.0.class_name);
                    for arg in &superclass.0.args {
                        self.mark_type_ast_nodes(arg);
                    }
                }
                for method in methods {
                    self.mark_ast_node(method.cst_id);
                    self.mark_type_ast_nodes(&method.ty);
                }
                for default in defaults {
                    self.mark_ast_node(default.cst_id);
                    for param in &default.params {
                        self.mark_pattern_ast_nodes(param);
                    }
                    self.mark_expr_ast_nodes(&default.body);
                }
            }
            SurfaceDefinition::InstanceDef {
                context,
                args,
                methods,
                ..
            } => {
                for predicate in context {
                    self.mark_ast_node(predicate.2);
                    for arg in &predicate.0.args {
                        self.mark_type_ast_nodes(arg);
                    }
                }
                for arg in args {
                    self.mark_type_ast_nodes(arg);
                }
                for method in methods {
                    self.mark_ast_node(method.cst_id);
                    for param in &method.params {
                        self.mark_pattern_ast_nodes(param);
                    }
                    self.mark_expr_ast_nodes(&method.body);
                }
            }
        }
    }

    fn mark_expr_ast_nodes(&mut self, expr: &Spanned<SurfaceExpr>) {
        self.mark_ast_node(expr.2);
        match &expr.0 {
            SurfaceExpr::App(lhs, rhs) | SurfaceExpr::OperatorApply(lhs, _, rhs) => {
                self.mark_expr_ast_nodes(lhs);
                self.mark_expr_ast_nodes(rhs);
            }
            SurfaceExpr::Abs(params, body) => {
                for (pattern, ty) in params {
                    self.mark_pattern_ast_nodes(pattern);
                    if let Some(ty) = ty {
                        self.mark_type_ast_nodes(ty);
                    }
                }
                self.mark_expr_ast_nodes(body);
            }
            SurfaceExpr::Ann(value, ty) => {
                self.mark_expr_ast_nodes(value);
                self.mark_type_ast_nodes(ty);
            }
            SurfaceExpr::Let(pattern, binder, body) => {
                self.mark_pattern_ast_nodes(pattern);
                self.mark_expr_ast_nodes(binder);
                self.mark_expr_ast_nodes(body);
            }
            SurfaceExpr::Case(scrutinee, arms) => {
                self.mark_expr_ast_nodes(scrutinee);
                for arm in arms {
                    self.mark_ast_node(arm.2);
                    self.mark_pattern_ast_nodes(&arm.0.pattern);
                    self.mark_expr_ast_nodes(&arm.0.expr);
                }
            }
            SurfaceExpr::Do(statements) => {
                for statement in statements {
                    self.mark_ast_node(statement.2);
                    match &statement.0 {
                        SurfaceDoStmt::Bind { pattern, expr }
                        | SurfaceDoStmt::Let { pattern, expr } => {
                            self.mark_pattern_ast_nodes(pattern);
                            self.mark_expr_ast_nodes(expr);
                        }
                        SurfaceDoStmt::Expr(expr) => self.mark_expr_ast_nodes(expr),
                    }
                }
            }
            SurfaceExpr::IfThenElse(cond, then_branch, else_branch) => {
                self.mark_expr_ast_nodes(cond);
                self.mark_expr_ast_nodes(then_branch);
                self.mark_expr_ast_nodes(else_branch);
            }
            SurfaceExpr::UnaryOperator(_, rhs)
            | SurfaceExpr::OperatorLeftSection(rhs, _)
            | SurfaceExpr::OperatorRightSection(_, rhs) => self.mark_expr_ast_nodes(rhs),
            SurfaceExpr::OpSeq(lhs, segments) => {
                self.mark_ast_node(lhs.cst_id);
                self.mark_expr_ast_nodes(&lhs.expr);
                for segment in segments {
                    self.mark_ast_node(segment.cst_id);
                    self.mark_ast_node(segment.rhs.cst_id);
                    self.mark_expr_ast_nodes(&segment.rhs.expr);
                }
            }
            SurfaceExpr::Var(name) | SurfaceExpr::Con(name) => self.mark_ast_node(name.2),
            SurfaceExpr::Unit | SurfaceExpr::Literal(_) | SurfaceExpr::Hole => {}
        }
    }

    fn mark_pattern_ast_nodes(&mut self, pattern: &Spanned<SurfacePattern>) {
        self.mark_ast_node(pattern.2);
        match &pattern.0 {
            SurfacePattern::Con(name, args) => {
                self.mark_ast_node(name.2);
                for arg in args {
                    self.mark_pattern_ast_nodes(arg);
                }
            }
            SurfacePattern::Var(_) | SurfacePattern::Lit(_) | SurfacePattern::Wild => {}
        }
    }

    fn mark_type_ast_nodes<T>(&mut self, ty: &Spanned<T>) {
        self.mark_ast_node(ty.2);
    }

    fn mark_ast_node(&mut self, id: CstId) {
        if let Some(slot) = self.ast_nodes.get_mut(id.index()) {
            *slot = true;
        }
    }

    fn nearest_ast_node(&self, node: CstId) -> Option<CstId> {
        let mut current = Some(node);
        while let Some(id) = current {
            if self.ast_nodes.get(id.index()).copied().unwrap_or(false) {
                return Some(id);
            }
            current = self.cst.parent(id);
        }

        None
    }

    fn classify_token(&self, token_id: TokenId, token: Token, span: Span) -> TargetKind {
        if token.is_trivia() {
            return TargetKind::Trivia;
        }
        if self.local_hovers.contains_key(&span) {
            return TargetKind::Local;
        }
        if let Some(target) = self.definitions.get(&span) {
            return TargetKind::Definition(*target);
        }
        if let Some(target) = self.references.get(&span) {
            return TargetKind::Reference(*target);
        }
        if let Some(target) = self.modules.get(&span) {
            return TargetKind::Module(*target);
        }
        if self.empty_list_token(token_id) {
            return TargetKind::Literal(LiteralTarget::EmptyList);
        }

        match token {
            Token::Int => TargetKind::Literal(LiteralTarget::Int),
            Token::Float => TargetKind::Literal(LiteralTarget::Float),
            Token::Char => TargetKind::Literal(LiteralTarget::Char),
            Token::Str => TargetKind::Literal(LiteralTarget::String),
            Token::Ident
            | Token::UpperIdent
            | Token::Operator
            | Token::Hole
            | Token::Underscore => TargetKind::Identifier,
            token if keyword_token(token) => TargetKind::Keyword,
            _ => TargetKind::Punctuation,
        }
    }

    fn empty_list_token(&self, token_id: TokenId) -> bool {
        let token_index = token_id.index();
        let Some(token) = self.cst.tokens().get(token_index) else {
            return false;
        };
        match token.kind {
            Token::LBrack => self
                .next_significant_token(token_index)
                .is_some_and(|next| {
                    self.cst
                        .tokens()
                        .get(next)
                        .is_some_and(|token| token.kind == Token::RBrack)
                }),
            Token::RBrack => self
                .previous_significant_token(token_index)
                .is_some_and(|previous| {
                    self.cst
                        .tokens()
                        .get(previous)
                        .is_some_and(|token| token.kind == Token::LBrack)
                }),
            _ => false,
        }
    }

    fn next_significant_token(&self, index: usize) -> Option<usize> {
        self.cst
            .tokens()
            .iter()
            .enumerate()
            .skip(index + 1)
            .find(|(_, token)| !token.is_synthetic() && !token.kind.is_trivia())
            .map(|(index, _)| index)
    }

    fn previous_significant_token(&self, index: usize) -> Option<usize> {
        self.cst
            .tokens()
            .get(..index)?
            .iter()
            .enumerate()
            .rev()
            .find(|(_, token)| !token.is_synthetic() && !token.kind.is_trivia())
            .map(|(index, _)| index)
    }

    fn insert_definition(&mut self, span: FileSpan, def_id: DefId) {
        if span.file != self.file {
            return;
        }

        let target = DefTarget { span, def_id };
        self.definitions.insert(span.span, target);
        for token_span in self.definition_token_spans(span.span) {
            self.definitions.entry(token_span).or_insert(target);
        }
    }

    fn insert_reference(&mut self, span: FileSpan, def_id: DefId) {
        if span.file != self.file {
            return;
        }

        let target = DefTarget { span, def_id };
        for token_span in self.reference_token_spans(span.span) {
            self.references.entry(token_span).or_insert(target);
        }
    }

    fn insert_module(&mut self, span: FileSpan, module_id: ModuleId) {
        if span.file != self.file {
            return;
        }

        let target = ModuleTarget { span, module_id };
        for token_span in self.reference_token_spans(span.span) {
            self.modules.insert(token_span, target);
        }
    }

    fn insert_local_hover(&mut self, hover: &HoverInfo) {
        if hover.file_span().file == self.file {
            let span = hover.file_span().span;
            self.local_hovers.insert(span, hover.clone());
            if let Some(ast) = self.tuple_pattern_nodes.get(&span).copied() {
                self.insert_expression_hover(ast, hover.clone());
            }
        }
    }

    fn insert_expression_hover(&mut self, ast: CstId, hover: HoverInfo) {
        if let Some(slot) = self.expression_hovers.get_mut(ast.index()) {
            slot.get_or_insert(hover);
        }
    }

    fn insert_signature_call_site(&mut self, ast: CstId, call_site: SignatureCallSite) {
        if let Some(slot) = self.signature_call_sites.get_mut(ast.index()) {
            slot.get_or_insert(call_site);
        }
    }

    fn reference_token_spans(&self, span: Span) -> Vec<Span> {
        self.token_spans_inside_name_span(span, false)
    }

    fn definition_token_spans(&self, span: Span) -> Vec<Span> {
        self.token_spans_inside_name_span(span, true)
    }

    fn token_spans_inside_name_span(&self, span: Span, include_delimiters: bool) -> Vec<Span> {
        let tokens = self
            .cst
            .tokens()
            .iter()
            .filter(|token| {
                !token.is_synthetic()
                    && span_contains_span(span, Span::from_usize_range(token.start()..token.end()))
            })
            .collect::<Vec<_>>();

        let (Some(first), Some(last)) = (tokens.first(), tokens.last()) else {
            return Vec::new();
        };
        if first.start() != span.start as usize || last.end() != span.end as usize {
            return Vec::new();
        }
        if !tokens
            .iter()
            .all(|token| !token.kind.is_trivia() && token_allowed_in_reference_span(token.kind))
        {
            return Vec::new();
        }

        tokens
            .into_iter()
            .filter(|token| {
                token_is_semantic_reference(token.kind)
                    || (include_delimiters && matches!(token.kind, Token::LParen | Token::RParen))
            })
            .filter_map(|token| {
                let token_span = Span::from_usize_range(token.start()..token.end());
                (!token_span.is_empty()).then_some(token_span)
            })
            .collect()
    }

    fn push_parent_node_spans(&self, mut node: Option<CstId>, spans: &mut Vec<FileSpan>) {
        while let Some(id) = node {
            let span = self.cst.node(id).span();
            if !span.is_empty() {
                spans.push(FileSpan::new(self.file, span));
            }
            node = self.cst.parent(id);
        }
    }
}

pub(crate) fn project_position_indexes(
    session: &Session,
    snapshot: &CompilationSnapshot,
    retained: &RetainedIdeArtifacts,
    resolutions: &FxHashMap<ModuleId, Vec<(FileSpan, DefId)>>,
    local_hovers: &[HoverInfo],
    ctor_table: &CtorTable,
    tables: PrettyTables<'_>,
) -> FxHashMap<FileId, PositionIndex> {
    let mut indexes = snapshot
        .project
        .modules
        .values()
        .map(|module_file| {
            (
                module_file.parsed.file,
                PositionIndex::new(&module_file.parsed),
            )
        })
        .collect::<FxHashMap<_, _>>();

    for (span, def_id) in &retained.declaration_spans {
        if let Some(index) = indexes.get_mut(&span.file) {
            index.insert_definition(*span, *def_id);
        }
    }

    for resolutions in resolutions.values() {
        for (span, def_id) in resolutions {
            if let Some(index) = indexes.get_mut(&span.file) {
                index.insert_reference(*span, *def_id);
            }
        }
    }

    for (span, module_id) in &retained.module_imports {
        let Some(index) = indexes.get_mut(&span.file) else {
            continue;
        };
        let Some(source) = snapshot
            .source_files
            .get(span.file.0 as usize)
            .map(|source| source.src.as_str())
        else {
            continue;
        };
        let module_span = retained
            .module_display_names
            .get(module_id)
            .and_then(|name| import_module_name_span(source, span.span, name))
            .unwrap_or(span.span);
        index.insert_module(FileSpan::new(span.file, module_span), *module_id);
    }

    for hover in local_hovers {
        if let Some(index) = indexes.get_mut(&hover.file_span().file) {
            index.insert_local_hover(hover);
        }
    }

    collect_expression_position_facts(
        session,
        snapshot,
        &retained.docs,
        ctor_table,
        tables,
        &mut indexes,
    );
    indexes
}

pub(crate) fn target_at_offset_in_artifacts(
    artifacts: &CompilationArtifacts,
    file: &Path,
    offset: usize,
) -> Option<PositionTarget> {
    let file_id = source_file_id(artifacts, file)?;
    artifacts.position_indexes.get(&file_id)?.target_at(offset)
}

pub(crate) fn target_at_position_in_artifacts(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
) -> Option<PositionTarget> {
    let file_id = source_file_id_in_source_map(source_map, file)?;
    let offset = source_map.offset(file_id, position, encoding) as usize;
    artifacts.position_indexes.get(&file_id)?.target_at(offset)
}

pub(crate) fn position_index_for_target(
    artifacts: &CompilationArtifacts,
    target: PositionTarget,
) -> Option<&PositionIndex> {
    artifacts.position_indexes.get(&target.file)
}

pub(crate) fn selection_ranges_at_positions_in_artifacts(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    positions: &[SourcePosition],
    encoding: PositionEncoding,
) -> Vec<SourceSelectionRange> {
    let Some(file_id) = source_file_id_in_source_map(source_map, file) else {
        return Vec::new();
    };
    let Some(index) = artifacts.position_indexes.get(&file_id) else {
        return Vec::new();
    };

    positions
        .iter()
        .map(|position| {
            let offset = source_map.offset(file_id, *position, encoding) as usize;
            index
                .selection_range_at(offset)
                .unwrap_or_else(|| empty_selection_range(file_id, offset))
        })
        .collect()
}

fn collect_expression_position_facts(
    session: &Session,
    snapshot: &CompilationSnapshot,
    docs: &FxHashMap<DefId, DocComment>,
    ctor_table: &CtorTable,
    tables: PrettyTables<'_>,
    indexes: &mut FxHashMap<FileId, PositionIndex>,
) {
    let cx = PositionFactContext {
        snapshot,
        docs,
        ctor_table,
        tables,
    };

    for (module_name, module_file) in &snapshot.project.modules {
        let Some(module_id) = snapshot.project.module_table.id(module_name) else {
            continue;
        };
        let Some(checked) = snapshot.checked.get(&module_id) else {
            continue;
        };
        let Some(index) = indexes.get_mut(&module_file.parsed.file) else {
            continue;
        };

        collect_module_position_facts(session, &module_file.parsed, module_id, checked, cx, index);
    }
}

fn collect_module_position_facts(
    session: &Session,
    parsed: &ParsedModule,
    module_id: ModuleId,
    checked: &CheckedModule,
    cx: PositionFactContext<'_>,
    index: &mut PositionIndex,
) {
    let checked_defs = checked
        .func_defs
        .iter()
        .map(|def| (def.id, def))
        .collect::<FxHashMap<_, _>>();

    let function_groups = function_equation_group_indices(parsed).unwrap_or_default();

    for group in function_groups {
        let Some(first_index) = group.first().copied() else {
            continue;
        };
        let Some(first_def) = parsed.defs.get(first_index) else {
            continue;
        };
        let SurfaceDefinition::FunDef { name, params, .. } = &first_def.0 else {
            continue;
        };
        let def_id = DefId {
            module: module_id,
            name: name.0,
        };
        let Some(checked_def) = checked_defs.get(&def_id).copied() else {
            continue;
        };
        collect_function_group_position_facts(parsed, &group, params.len(), checked_def, cx, index);
    }

    let mut checked_instances = checked.instance_defs.iter();
    for definition in &parsed.defs {
        let SurfaceDefinition::InstanceDef { methods, .. } = &definition.0 else {
            continue;
        };
        let Some(checked_instance) = checked_instances.next() else {
            continue;
        };

        let method_groups =
            instance_method_equation_group_indices(parsed.file, methods).unwrap_or_default();
        for group in method_groups {
            let Some(first_method) = group.first().and_then(|index| methods.get(*index)) else {
                continue;
            };
            let method_name = session.str(first_method.name.0);
            let Some(checked_method) = checked_instance
                .methods
                .iter()
                .find(|checked_method| checked_method.name == *method_name)
            else {
                continue;
            };
            drop(method_name);

            collect_instance_method_group_position_facts(
                methods,
                &group,
                checked_method,
                cx,
                index,
            );
        }
    }
}

fn collect_function_group_position_facts(
    parsed: &ParsedModule,
    group: &[usize],
    param_count: usize,
    checked_def: &TermDefinition<TcMetadata, Range<usize>>,
    cx: PositionFactContext<'_>,
    index: &mut PositionIndex,
) {
    let checked_body = checked_body_without_function_params(&checked_def.body, param_count);

    if group.len() == 1
        && !matches!(checked_body.node, TermExpr::Match(_, _))
        && let Some(first_index) = group.first().copied()
        && let Some(defn) = parsed.defs.get(first_index)
        && let SurfaceDefinition::FunDef { body, .. } = &defn.0
    {
        collect_expr_position_facts(
            body,
            checked_body,
            cx.snapshot,
            cx.docs,
            cx.ctor_table,
            cx.tables,
            index,
        );
        return;
    }

    let TermExpr::Match(_, arms) = &checked_body.node else {
        return;
    };

    for (def_index, checked_arm) in group.iter().zip(arms) {
        let Some(defn) = parsed.defs.get(*def_index) else {
            continue;
        };
        let SurfaceDefinition::FunDef { body, .. } = &defn.0 else {
            continue;
        };
        collect_expr_position_facts(
            body,
            &checked_arm.node.expr,
            cx.snapshot,
            cx.docs,
            cx.ctor_table,
            cx.tables,
            index,
        );
    }
}

fn collect_expr_position_facts(
    surface: &Spanned<SurfaceExpr>,
    checked: &TermExprNode<TcMetadata, Range<usize>>,
    snapshot: &CompilationSnapshot,
    docs: &FxHashMap<DefId, DocComment>,
    ctor_table: &CtorTable,
    tables: PrettyTables<'_>,
    index: &mut PositionIndex,
) {
    let cx = PositionFactContext {
        snapshot,
        docs,
        ctor_table,
        tables,
    };
    let checked = strip_checked_wrappers(checked);
    if !surface.1.is_empty() {
        index.insert_expression_hover(
            surface.2,
            HoverInfo::new(
                index.file,
                surface.1.to_usize_range(),
                checked.anno.inferred_type.pretty_with_tables(tables),
                None,
            ),
        );
    }

    if let TermExpr::Anno(checked_value, _) = &checked.node
        && !matches!(surface.0, SurfaceExpr::Ann(_, _))
    {
        collect_expr_position_facts(
            surface,
            checked_value,
            snapshot,
            docs,
            ctor_table,
            tables,
            index,
        );
        return;
    }

    match (&surface.0, &checked.node) {
        (SurfaceExpr::Abs(params, body), _) => {
            if let Some(checked_body) = checked_body_after_lambda_params(params, checked) {
                collect_expr_position_facts(
                    body,
                    checked_body,
                    snapshot,
                    docs,
                    ctor_table,
                    tables,
                    index,
                );
            }
        }
        (SurfaceExpr::Let(_, binder, body), TermExpr::Let(_, checked_binder, checked_body)) => {
            collect_expr_position_facts(
                binder,
                checked_binder,
                snapshot,
                docs,
                ctor_table,
                tables,
                index,
            );
            collect_expr_position_facts(
                body,
                checked_body,
                snapshot,
                docs,
                ctor_table,
                tables,
                index,
            );
        }
        (SurfaceExpr::Let(_, binder, body), TermExpr::Case(checked_binder, checked_arms))
            if checked_arms.len() == 1 =>
        {
            let Some(checked_arm) = checked_arms.first() else {
                return;
            };
            collect_expr_position_facts(
                binder,
                checked_binder,
                snapshot,
                docs,
                ctor_table,
                tables,
                index,
            );
            collect_expr_position_facts(
                body,
                &checked_arm.node.expr,
                snapshot,
                docs,
                ctor_table,
                tables,
                index,
            );
        }
        (SurfaceExpr::App(surface_func, surface_arg), TermExpr::App(checked_func, checked_arg)) => {
            maybe_insert_signature_call_site(
                surface.2,
                surface.1.to_usize_range(),
                checked,
                cx,
                index,
            );
            collect_expr_position_facts(
                surface_func,
                checked_func,
                snapshot,
                docs,
                ctor_table,
                tables,
                index,
            );
            collect_expr_position_facts(
                surface_arg,
                checked_arg,
                snapshot,
                docs,
                ctor_table,
                tables,
                index,
            );
        }
        (SurfaceExpr::Ann(value, _), TermExpr::Anno(checked_value, _)) => {
            collect_expr_position_facts(
                value,
                checked_value,
                snapshot,
                docs,
                ctor_table,
                tables,
                index,
            );
        }
        (SurfaceExpr::Ann(value, _), _) => {
            collect_expr_position_facts(value, checked, snapshot, docs, ctor_table, tables, index);
        }
        (
            SurfaceExpr::IfThenElse(surface_cond, surface_then, surface_else),
            TermExpr::IfThenElse(checked_cond, checked_then, checked_else),
        ) => {
            collect_expr_position_facts(
                surface_cond,
                checked_cond,
                snapshot,
                docs,
                ctor_table,
                tables,
                index,
            );
            collect_expr_position_facts(
                surface_then,
                checked_then,
                snapshot,
                docs,
                ctor_table,
                tables,
                index,
            );
            collect_expr_position_facts(
                surface_else,
                checked_else,
                snapshot,
                docs,
                ctor_table,
                tables,
                index,
            );
        }
        (
            SurfaceExpr::Case(surface_scrutinee, surface_arms),
            TermExpr::Case(checked_scrutinee, checked_arms),
        ) => {
            collect_expr_position_facts(
                surface_scrutinee,
                checked_scrutinee,
                snapshot,
                docs,
                ctor_table,
                tables,
                index,
            );
            for (surface_arm, checked_arm) in surface_arms.iter().zip(checked_arms) {
                collect_expr_position_facts(
                    &surface_arm.0.expr,
                    &checked_arm.node.expr,
                    snapshot,
                    docs,
                    ctor_table,
                    tables,
                    index,
                );
            }
        }
        (SurfaceExpr::Do(statements), _) => {
            collect_do_position_facts(
                statements, checked, snapshot, docs, ctor_table, tables, index,
            );
        }
        (
            SurfaceExpr::OperatorApply(surface_lhs, _, surface_rhs),
            TermExpr::App(checked_func, checked_rhs),
        ) => {
            maybe_insert_signature_call_site(
                surface.2,
                surface.1.to_usize_range(),
                checked,
                cx,
                index,
            );
            if let TermExpr::App(_, checked_lhs) = &strip_checked_wrappers(checked_func).node {
                collect_expr_position_facts(
                    surface_lhs,
                    checked_lhs,
                    snapshot,
                    docs,
                    ctor_table,
                    tables,
                    index,
                );
            }
            collect_expr_position_facts(
                surface_rhs,
                checked_rhs,
                snapshot,
                docs,
                ctor_table,
                tables,
                index,
            );
        }
        (SurfaceExpr::UnaryOperator(_, surface_rhs), TermExpr::App(_, checked_rhs)) => {
            maybe_insert_signature_call_site(
                surface.2,
                surface.1.to_usize_range(),
                checked,
                cx,
                index,
            );
            collect_expr_position_facts(
                surface_rhs,
                checked_rhs,
                snapshot,
                docs,
                ctor_table,
                tables,
                index,
            );
        }
        (SurfaceExpr::OpSeq(lhs, segments), _) => {
            maybe_insert_signature_call_site(
                surface.2,
                surface.1.to_usize_range(),
                checked,
                cx,
                index,
            );
            collect_opseq_position_facts(lhs, segments, checked, cx, index);
        }
        _ => {}
    }
}

fn collect_opseq_position_facts(
    lhs: &SurfaceOpOperand,
    segments: &[SurfaceOpSegment],
    checked: &CheckedExpr,
    cx: PositionFactContext<'_>,
    index: &mut PositionIndex,
) {
    collect_op_operand_position_facts(lhs, checked, cx, index);
    for segment in segments {
        collect_op_operand_position_facts(&segment.rhs, checked, cx, index);
    }
}

fn maybe_insert_signature_call_site(
    ast: CstId,
    source_span: Range<usize>,
    checked: &CheckedExpr,
    cx: PositionFactContext<'_>,
    index: &mut PositionIndex,
) {
    if let Some(call_site) = signature_call_site_for_application(source_span, cx, checked) {
        index.insert_signature_call_site(ast, call_site);
    }
}

fn signature_call_site_for_application(
    source_span: Range<usize>,
    cx: PositionFactContext<'_>,
    checked: &CheckedExpr,
) -> Option<SignatureCallSite> {
    if !matches!(strip_checked_wrappers(checked).node, TermExpr::App(_, _)) {
        return None;
    }

    let (head, args) = flatten_application(checked);
    if args.is_empty() {
        return None;
    }

    let name = callable_name(cx.snapshot, head)?;
    let signature_type = callable_type(cx.snapshot, cx.ctor_table, head);
    let parameters = arrow_parameter_types(&signature_type, cx.tables);
    if parameters.is_empty() {
        return None;
    }

    let label = format!(
        "{name} : {}",
        source_signature::pretty_source_ty(&signature_type, cx.tables)
    );
    let signature = SourceSignatureInformation::new(label, parameters)
        .with_optional_documentation(callable_documentation(cx.docs, head));
    let argument_spans = args
        .into_iter()
        .map(|arg| arg.anno.span.clone())
        .collect::<Vec<_>>();

    Some(SignatureCallSite {
        source_span,
        signature,
        argument_spans,
    })
}

fn flatten_application(expr: &CheckedExpr) -> (CheckedExprRef<'_>, Vec<CheckedExprRef<'_>>) {
    let mut current = expr;
    let mut args = Vec::new();

    while let TermExpr::App(func, arg) = &strip_checked_wrappers(current).node {
        args.push(&**arg);
        current = func;
    }

    args.reverse();
    (current, args)
}

fn callable_name(
    snapshot: &CompilationSnapshot,
    head: &TermExprNode<TcMetadata, Range<usize>>,
) -> Option<String> {
    match &strip_checked_wrappers(head).node {
        TermExpr::Var(var) => Some(var.display_name(&snapshot.names)),
        TermExpr::Con(id) => Some(snapshot.names.ctor_display_name(*id)),
        _ => None,
    }
}

fn callable_type(
    snapshot: &CompilationSnapshot,
    ctor_table: &CtorTable,
    head: &TermExprNode<TcMetadata, Range<usize>>,
) -> Ty {
    match &strip_checked_wrappers(head).node {
        TermExpr::Var(VarRef::Global(def_id)) => global_callable_type(snapshot, *def_id)
            .unwrap_or_else(|| head.anno.inferred_type.clone()),
        TermExpr::Con(id) => ctor_table.get(*id).map_or_else(
            || head.anno.inferred_type.clone(),
            |info| info.full_type.clone(),
        ),
        _ => head.anno.inferred_type.clone(),
    }
}

fn global_callable_type(snapshot: &CompilationSnapshot, def_id: DefId) -> Option<Ty> {
    for checked in snapshot.checked.values() {
        if let Some(scheme) = checked.func_schemes.get(&def_id) {
            return Some(scheme.to_ty());
        }

        for class_def in &checked.class_defs {
            if let Some(method) = class_def.methods.iter().find(|method| method.id == def_id) {
                return Some(method.scheme.to_ty());
            }
        }
    }

    None
}

fn callable_documentation(
    docs: &FxHashMap<DefId, DocComment>,
    head: &TermExprNode<TcMetadata, Range<usize>>,
) -> Option<String> {
    match &strip_checked_wrappers(head).node {
        TermExpr::Var(VarRef::Global(def_id)) => docs.get(def_id).map(|doc| doc.text.clone()),
        _ => None,
    }
}

fn arrow_parameter_types(ty: &Ty, tables: PrettyTables<'_>) -> Vec<String> {
    let (_, _, mut current) = ty.peel_scheme_like();
    let mut params = Vec::new();

    while let Ty::TyArr(param, rest) = current {
        params.push(source_signature::pretty_source_ty(&param, tables));
        current = *rest;
    }

    params
}

fn collect_op_operand_position_facts(
    operand: &SurfaceOpOperand,
    checked: &CheckedExpr,
    cx: PositionFactContext<'_>,
    index: &mut PositionIndex,
) {
    if let Some(checked_operand) = checked_subexpr_for_exact_span(checked, operand.expr.1) {
        collect_expr_position_facts(
            &operand.expr,
            checked_operand,
            cx.snapshot,
            cx.docs,
            cx.ctor_table,
            cx.tables,
            index,
        );
    }
}

fn checked_subexpr_for_exact_span(
    expr: &TermExprNode<TcMetadata, Range<usize>>,
    span: Span,
) -> Option<&TermExprNode<TcMetadata, Range<usize>>> {
    let expr = strip_checked_wrappers(expr);
    let range = span.to_usize_range();
    if expr.anno.span == range {
        return Some(expr);
    }

    match &expr.node {
        TermExpr::App(func, arg) => checked_subexpr_for_exact_span(func, span)
            .or_else(|| checked_subexpr_for_exact_span(arg, span)),
        TermExpr::Abs(_, _, body)
        | TermExpr::TyAbs(_, _, body)
        | TermExpr::TyApp(body, _)
        | TermExpr::EvAbs(_, _, body)
        | TermExpr::EvApp(body, _)
        | TermExpr::Anno(body, _) => checked_subexpr_for_exact_span(body, span),
        TermExpr::Case(scrutinee, arms) => {
            checked_subexpr_for_exact_span(scrutinee, span).or_else(|| {
                arms.iter()
                    .find_map(|arm| checked_subexpr_for_exact_span(&arm.node.expr, span))
            })
        }
        TermExpr::Match(scrutinees, arms) => scrutinees
            .iter()
            .find_map(|scrutinee| checked_subexpr_for_exact_span(scrutinee, span))
            .or_else(|| {
                arms.iter()
                    .find_map(|arm| checked_subexpr_for_exact_span(&arm.node.expr, span))
            }),
        TermExpr::IfThenElse(cond, then_branch, else_branch) => {
            checked_subexpr_for_exact_span(cond, span)
                .or_else(|| checked_subexpr_for_exact_span(then_branch, span))
                .or_else(|| checked_subexpr_for_exact_span(else_branch, span))
        }
        TermExpr::Let(_, binder, body) => checked_subexpr_for_exact_span(binder, span)
            .or_else(|| checked_subexpr_for_exact_span(body, span)),
        TermExpr::Unit
        | TermExpr::Var(_)
        | TermExpr::Lit(_)
        | TermExpr::Hole
        | TermExpr::Con(_) => None,
    }
}

fn collect_do_position_facts(
    statements: &[Spanned<SurfaceDoStmt>],
    checked: &TermExprNode<TcMetadata, Range<usize>>,
    snapshot: &CompilationSnapshot,
    docs: &FxHashMap<DefId, DocComment>,
    ctor_table: &CtorTable,
    tables: PrettyTables<'_>,
    index: &mut PositionIndex,
) {
    let checked = strip_checked_wrappers(checked);
    let Some((statement, rest)) = statements.split_first() else {
        return;
    };

    match (&statement.0, rest.is_empty()) {
        (SurfaceDoStmt::Expr(expr), true) => {
            collect_expr_position_facts(expr, checked, snapshot, docs, ctor_table, tables, index);
        }
        (SurfaceDoStmt::Expr(expr), false) => {
            if let Some((checked_expr, checked_rest)) = checked_do_application_parts(checked) {
                collect_expr_position_facts(
                    expr,
                    checked_expr,
                    snapshot,
                    docs,
                    ctor_table,
                    tables,
                    index,
                );
                collect_do_position_facts(
                    rest,
                    checked_rest,
                    snapshot,
                    docs,
                    ctor_table,
                    tables,
                    index,
                );
            }
        }
        (SurfaceDoStmt::Bind { pattern, expr }, false) => {
            if let Some((checked_expr, checked_rest)) = checked_do_application_parts(checked) {
                collect_expr_position_facts(
                    expr,
                    checked_expr,
                    snapshot,
                    docs,
                    ctor_table,
                    tables,
                    index,
                );
                collect_do_position_facts(
                    rest,
                    checked_do_rest_after_continuation(checked_rest, pattern),
                    snapshot,
                    docs,
                    ctor_table,
                    tables,
                    index,
                );
            }
        }
        (SurfaceDoStmt::Let { pattern: _, expr }, false) => {
            if let TermExpr::Let(_, checked_expr, checked_rest) = &checked.node {
                collect_expr_position_facts(
                    expr,
                    checked_expr,
                    snapshot,
                    docs,
                    ctor_table,
                    tables,
                    index,
                );
                collect_do_position_facts(
                    rest,
                    checked_rest,
                    snapshot,
                    docs,
                    ctor_table,
                    tables,
                    index,
                );
            } else if let TermExpr::Case(checked_expr, checked_arms) = &checked.node
                && checked_arms.len() == 1
            {
                let Some(checked_arm) = checked_arms.first() else {
                    return;
                };
                collect_expr_position_facts(
                    expr,
                    checked_expr,
                    snapshot,
                    docs,
                    ctor_table,
                    tables,
                    index,
                );
                collect_do_position_facts(
                    rest,
                    &checked_arm.node.expr,
                    snapshot,
                    docs,
                    ctor_table,
                    tables,
                    index,
                );
            }
        }
        (SurfaceDoStmt::Bind { .. } | SurfaceDoStmt::Let { .. }, true) => {}
    }
}

fn checked_do_application_parts(
    checked: &CheckedExpr,
) -> Option<(CheckedExprRef<'_>, CheckedExprRef<'_>)> {
    let TermExpr::App(checked_func, checked_rest) = &strip_checked_wrappers(checked).node else {
        return None;
    };
    let TermExpr::App(_, checked_expr) = &strip_checked_wrappers(checked_func).node else {
        return None;
    };

    Some((checked_expr, checked_rest))
}

fn checked_do_rest_after_continuation<'a>(
    checked: &'a TermExprNode<TcMetadata, Range<usize>>,
    pattern: &Spanned<SurfacePattern>,
) -> &'a TermExprNode<TcMetadata, Range<usize>> {
    let checked = strip_checked_wrappers(checked);
    if let TermExpr::Abs(_, _, body) = &checked.node {
        let body = strip_checked_wrappers(body);
        if !matches!(pattern.0, SurfacePattern::Var(_) | SurfacePattern::Wild)
            && let TermExpr::Case(_, arms) = &body.node
            && let [arm] = arms.as_slice()
        {
            return strip_checked_wrappers(&arm.node.expr);
        }
        return body;
    }

    checked
}

fn checked_body_after_lambda_params<'a>(
    params: &[(Spanned<SurfacePattern>, Option<Spanned<SurfaceTy>>)],
    checked: &'a CheckedExpr,
) -> Option<&'a CheckedExpr> {
    let mut current = checked;

    for (pattern, _) in params {
        current = strip_checked_wrappers(current);
        let TermExpr::Abs(_, _, body) = &current.node else {
            return None;
        };
        current = body;
        if !matches!(pattern.0, SurfacePattern::Var(_) | SurfacePattern::Wild) {
            let TermExpr::Case(_, arms) = &strip_checked_wrappers(current).node else {
                return None;
            };
            let [arm] = arms.as_slice() else {
                return None;
            };
            current = &arm.node.expr;
        }
    }

    Some(strip_checked_wrappers(current))
}

fn checked_body_without_function_params(
    body: &TermExprNode<TcMetadata, Range<usize>>,
    param_count: usize,
) -> &TermExprNode<TcMetadata, Range<usize>> {
    let mut current = strip_checked_wrappers(body);

    for _ in 0..param_count {
        let TermExpr::Abs(_, _, body) = &current.node else {
            break;
        };
        current = strip_checked_wrappers(body);
    }

    current
}

fn strip_checked_wrappers(
    expr: &TermExprNode<TcMetadata, Range<usize>>,
) -> &TermExprNode<TcMetadata, Range<usize>> {
    let mut current = expr;

    loop {
        current = match &current.node {
            TermExpr::TyAbs(_, _, body)
            | TermExpr::TyApp(body, _)
            | TermExpr::EvAbs(_, _, body)
            | TermExpr::EvApp(body, _) => body,
            _ => return current,
        };
    }
}

fn empty_selection_range(file: FileId, offset: usize) -> SourceSelectionRange {
    SourceSelectionRange::new(
        FileSpan::new(file, Span::from_usize_range(offset..offset)),
        None,
    )
}

fn keyword_token(token: Token) -> bool {
    matches!(
        token,
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
            | Token::Export
            | Token::Private
            | Token::Reexport
            | Token::As
            | Token::Infixl
            | Token::Infixr
            | Token::Infix
    )
}

fn token_is_semantic_reference(token: Token) -> bool {
    matches!(
        token,
        Token::Ident | Token::UpperIdent | Token::Operator | Token::Minus | Token::Cons
    )
}

fn token_allowed_in_reference_span(token: Token) -> bool {
    token_is_semantic_reference(token)
        || matches!(
            token,
            Token::Dot | Token::Backtick | Token::LParen | Token::RParen
        )
}

fn span_contains_span(parent: Span, child: Span) -> bool {
    parent.start <= child.start && child.end <= parent.end
}

fn span_contains_offset_inclusive(span: &Range<usize>, offset: usize) -> bool {
    if span.start == span.end {
        offset == span.start
    } else {
        span.start <= offset && offset <= span.end
    }
}
