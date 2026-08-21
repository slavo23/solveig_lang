//! Source-level completion collection and filtering.

use super::*;

mod typed;

pub(super) use typed::{
    TypedModuleCompletionInput, typed_local_completion_candidates,
    typed_module_completion_candidates,
};

const KEYWORDS: [&str; 18] = [
    "as", "case", "class", "data", "do", "else", "forall", "if", "import", "in", "infix", "infixl",
    "infixr", "instance", "let", "of", "then", "where",
];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CompletionKey {
    label: String,
    kind: SourceCompletionItemKind,
}

struct CompletionContext {
    qualifier: Option<String>,
}

pub(super) fn completion_items_in_compilation(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    candidates: &[SourceCompletionCandidate],
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
) -> Vec<SourceCompletionItem> {
    let Some(target) =
        position::target_at_position_in_artifacts(artifacts, source_map, file, position, encoding)
    else {
        return Vec::new();
    };
    let context = completion_context(source_map, target.file, target.offset);
    let mut by_key = BTreeMap::<CompletionKey, SourceCompletionItem>::new();

    for candidate in candidates {
        if !candidate.is_visible_at(
            target.file,
            u32::try_from(target.offset).unwrap_or(u32::MAX),
            context.qualifier.as_deref(),
        ) {
            continue;
        }

        let item = candidate.item().clone();
        let key = CompletionKey {
            label: item.label().to_string(),
            kind: item.kind(),
        };

        by_key
            .entry(key)
            .and_modify(|existing| {
                if completion_quality(&item) > completion_quality(existing) {
                    *existing = item.clone();
                }
            })
            .or_insert(item);
    }

    let mut items = by_key.into_values().collect::<Vec<_>>();
    items.sort_by_key(|item| (completion_kind_rank(item.kind()), item.label().to_string()));
    items
}

pub(super) fn syntax_completion_candidates(
    session: &Session,
    project: &Project,
) -> Vec<SourceCompletionCandidate> {
    let mut out = KEYWORDS
        .iter()
        .map(|keyword| {
            SourceCompletionCandidate::new(SourceCompletionItem::new(
                *keyword,
                SourceCompletionItemKind::Keyword,
            ))
        })
        .collect::<Vec<_>>();

    for module_file in project.modules.values() {
        push_module_path_candidates(session, &module_file.name, &mut out);
        push_surface_declaration_candidates(session, module_file, &mut out);
        push_surface_local_candidates(session, module_file, &mut out);
    }

    out
}

fn completion_quality(item: &SourceCompletionItem) -> u8 {
    u8::from(item.detail().is_some()) + u8::from(item.documentation().is_some())
}

fn completion_kind_rank(kind: SourceCompletionItemKind) -> u8 {
    match kind {
        SourceCompletionItemKind::Variable => 0,
        SourceCompletionItemKind::Function | SourceCompletionItemKind::Method => 1,
        SourceCompletionItemKind::Constructor => 2,
        SourceCompletionItemKind::Type | SourceCompletionItemKind::Class => 3,
        SourceCompletionItemKind::Module => 4,
        SourceCompletionItemKind::Keyword => 5,
    }
}

fn completion_context(source_map: &SourceMap, file: FileId, offset: usize) -> CompletionContext {
    let Some(source) = source_map.files().get(file.0 as usize) else {
        return CompletionContext { qualifier: None };
    };
    let offset = offset.min(source.src.len());
    let Some(prefix) = source.src.get(..offset) else {
        return CompletionContext { qualifier: None };
    };
    let start = prefix
        .rfind(|ch: char| !completion_path_char(ch))
        .map_or(0, |index| index + 1);
    let Some(path) = source.src.get(start..offset) else {
        return CompletionContext { qualifier: None };
    };
    let qualifier = path.rfind('.').and_then(|dot| {
        let qualifier = path.get(..dot)?;
        (!qualifier.is_empty()).then(|| qualifier.to_string())
    });

    CompletionContext { qualifier }
}

fn completion_path_char(ch: char) -> bool {
    ch == '.' || ch == '_' || ch.is_ascii_alphanumeric()
}

fn push_module_path_candidates(
    session: &Session,
    module_name: &ModuleName,
    out: &mut Vec<SourceCompletionCandidate>,
) {
    for index in 1..module_name.0.len() {
        let qualifier = module_name
            .0
            .get(..index)
            .expect("ICE: module path prefix index out of bounds")
            .iter()
            .map(|segment| session.str(*segment).to_string())
            .collect::<Vec<_>>()
            .join(".");
        let label = session
            .str(
                *module_name
                    .0
                    .get(index)
                    .expect("ICE: module path segment index out of bounds"),
            )
            .to_string();

        out.push(
            SourceCompletionCandidate::new(SourceCompletionItem::new(
                label,
                SourceCompletionItemKind::Module,
            ))
            .qualified(qualifier),
        );
    }
}

fn push_surface_declaration_candidates(
    session: &Session,
    module_file: &ModuleFile,
    out: &mut Vec<SourceCompletionCandidate>,
) {
    for defn in &module_file.parsed.defs {
        match &defn.0 {
            SurfaceDefinition::FunDef { name, docs, .. }
            | SurfaceDefinition::SigDef { name, docs, .. } => {
                push_surface_candidate(
                    session,
                    module_file.parsed.file,
                    name.0,
                    SourceCompletionItemKind::Function,
                    docs.as_ref(),
                    out,
                );
            }
            SurfaceDefinition::DataDef {
                name,
                constructors,
                docs,
                ..
            } => {
                push_surface_candidate(
                    session,
                    module_file.parsed.file,
                    name.0,
                    SourceCompletionItemKind::Type,
                    docs.as_ref(),
                    out,
                );
                for constructor in constructors {
                    push_surface_candidate(
                        session,
                        module_file.parsed.file,
                        constructor.0.name.0,
                        SourceCompletionItemKind::Constructor,
                        constructor.0.docs.as_ref(),
                        out,
                    );
                }
            }
            SurfaceDefinition::TypeAliasDef { name, docs, .. } => {
                push_surface_candidate(
                    session,
                    module_file.parsed.file,
                    name.0,
                    SourceCompletionItemKind::Type,
                    docs.as_ref(),
                    out,
                );
            }
            SurfaceDefinition::ClassDef {
                name,
                methods,
                docs,
                ..
            } => {
                push_surface_candidate(
                    session,
                    module_file.parsed.file,
                    name.0,
                    SourceCompletionItemKind::Class,
                    docs.as_ref(),
                    out,
                );
                for method in methods {
                    push_surface_candidate(
                        session,
                        module_file.parsed.file,
                        method.name.0,
                        SourceCompletionItemKind::Method,
                        method.docs.as_ref(),
                        out,
                    );
                }
            }
            SurfaceDefinition::FixityDef { .. } | SurfaceDefinition::InstanceDef { .. } => {}
        }
    }
}

fn push_surface_candidate(
    session: &Session,
    file: FileId,
    name: Symbol,
    kind: SourceCompletionItemKind,
    docs: Option<&DocComment>,
    out: &mut Vec<SourceCompletionCandidate>,
) {
    out.push(
        SourceCompletionCandidate::new(
            SourceCompletionItem::new(session.str(name).to_string(), kind)
                .with_optional_documentation(docs.map(|doc| doc.text.clone())),
        )
        .in_file(file),
    );
}

fn push_surface_local_candidates(
    session: &Session,
    module_file: &ModuleFile,
    out: &mut Vec<SourceCompletionCandidate>,
) {
    for defn in &module_file.parsed.defs {
        match &defn.0 {
            SurfaceDefinition::FunDef { params, body, .. } => {
                let mut binders = Vec::new();
                for param in params {
                    collect_pattern_binders(param, &mut binders);
                }
                for binder in binders {
                    push_local_candidate(session, module_file.parsed.file, binder, body.1, out);
                }
                collect_surface_expr_locals(session, module_file.parsed.file, body, out);
            }
            SurfaceDefinition::InstanceDef { methods, .. } => {
                for method in methods {
                    let mut binders = Vec::new();
                    for param in &method.params {
                        collect_pattern_binders(param, &mut binders);
                    }
                    for binder in binders {
                        push_local_candidate(
                            session,
                            module_file.parsed.file,
                            binder,
                            method.body.1,
                            out,
                        );
                    }
                    collect_surface_expr_locals(
                        session,
                        module_file.parsed.file,
                        &method.body,
                        out,
                    );
                }
            }
            _ => {}
        }
    }
}

fn collect_surface_expr_locals(
    session: &Session,
    file: FileId,
    expr: &Spanned<SurfaceExpr>,
    out: &mut Vec<SourceCompletionCandidate>,
) {
    match &expr.0 {
        SurfaceExpr::Abs(params, body) => {
            let mut binders = Vec::new();
            for (pattern, _) in params {
                collect_pattern_binders(pattern, &mut binders);
            }
            for binder in binders {
                push_local_candidate(session, file, binder, body.1, out);
            }
            collect_surface_expr_locals(session, file, body, out);
        }
        SurfaceExpr::Let(pattern, binder, in_expr) => {
            collect_surface_expr_locals(session, file, binder, out);
            let mut binders = Vec::new();
            collect_pattern_binders(pattern, &mut binders);
            for binder in binders {
                push_local_candidate(session, file, binder, in_expr.1, out);
            }
            collect_surface_expr_locals(session, file, in_expr, out);
        }
        SurfaceExpr::App(lhs, rhs) | SurfaceExpr::OperatorApply(lhs, _, rhs) => {
            collect_surface_expr_locals(session, file, lhs, out);
            collect_surface_expr_locals(session, file, rhs, out);
        }
        SurfaceExpr::OpSeq(lhs, segments) => {
            collect_surface_expr_locals(session, file, &lhs.expr, out);
            for segment in segments {
                collect_surface_expr_locals(session, file, &segment.rhs.expr, out);
            }
        }
        SurfaceExpr::Ann(inner, _)
        | SurfaceExpr::UnaryOperator(_, inner)
        | SurfaceExpr::OperatorLeftSection(inner, _)
        | SurfaceExpr::OperatorRightSection(_, inner) => {
            collect_surface_expr_locals(session, file, inner, out);
        }
        SurfaceExpr::Case(scrutinee, arms) => {
            collect_surface_expr_locals(session, file, scrutinee, out);
            for arm in arms {
                let mut binders = Vec::new();
                collect_pattern_binders(&arm.0.pattern, &mut binders);
                for binder in binders {
                    push_local_candidate(session, file, binder, arm.0.expr.1, out);
                }
                collect_surface_expr_locals(session, file, &arm.0.expr, out);
            }
        }
        SurfaceExpr::Do(statements) => {
            for statement in statements {
                match &statement.0 {
                    SurfaceDoStmt::Bind { expr, .. }
                    | SurfaceDoStmt::Let { expr, .. }
                    | SurfaceDoStmt::Expr(expr) => {
                        collect_surface_expr_locals(session, file, expr, out);
                    }
                }
            }

            for (index, statement) in statements.iter().enumerate() {
                let scope_end = statements.last().map_or(expr.1.end, |last| last.1.end);
                match &statement.0 {
                    SurfaceDoStmt::Bind { pattern, .. } | SurfaceDoStmt::Let { pattern, .. } => {
                        let scope_start = statements
                            .get(index + 1)
                            .map_or(statement.1.end, |next| next.1.start);
                        let mut binders = Vec::new();
                        collect_pattern_binders(pattern, &mut binders);
                        for binder in binders {
                            push_local_candidate(
                                session,
                                file,
                                binder,
                                Span::new(scope_start, scope_end),
                                out,
                            );
                        }
                    }
                    SurfaceDoStmt::Expr(_) => {}
                }
            }
        }
        SurfaceExpr::IfThenElse(cond, then_branch, else_branch) => {
            collect_surface_expr_locals(session, file, cond, out);
            collect_surface_expr_locals(session, file, then_branch, out);
            collect_surface_expr_locals(session, file, else_branch, out);
        }
        SurfaceExpr::Unit
        | SurfaceExpr::Literal(_)
        | SurfaceExpr::Hole
        | SurfaceExpr::Var(_)
        | SurfaceExpr::Con(_) => {}
    }
}

fn collect_pattern_binders(pattern: &Spanned<SurfacePattern>, out: &mut Vec<Symbol>) {
    match &pattern.0 {
        SurfacePattern::Var(symbol) => out.push(*symbol),
        SurfacePattern::Con(_, args) => {
            for arg in args {
                collect_pattern_binders(arg, out);
            }
        }
        SurfacePattern::Lit(_) | SurfacePattern::Wild => {}
    }
}

fn push_local_candidate(
    session: &Session,
    file: FileId,
    name: Symbol,
    scope: Span,
    out: &mut Vec<SourceCompletionCandidate>,
) {
    out.push(
        SourceCompletionCandidate::new(SourceCompletionItem::new(
            session.str(name).to_string(),
            SourceCompletionItemKind::Variable,
        ))
        .in_scope(file, scope),
    );
}
