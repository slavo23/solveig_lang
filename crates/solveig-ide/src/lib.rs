//! Protocol-agnostic editor features for Solveig projects.
//! This crate derives hover, navigation, completion, and related editor data from
//! retained driver snapshots without depending on LSP protocol types.

use solveig_ast::ast::{
    ClassDefinition, ClassId, ClassInfo, ClassTable, CtorId, CtorTable, DataDefinition, DefId,
    InstanceDefinition, InstanceMethodDef, Kind, ModuleId, Names, PRELUDE_LIST_NAME, PRIM_TYS,
    Pattern, PatternNode, Predicate, Pretty, PrettyTables, Scheme, TermDefinition, TermExpr,
    TermExprNode, Ty, TyConDefinition, TyConId, TyConInfo, TyConTable, TyVarId, VarRef,
    compiler_bool_data_definition, tuple_constructor_spelling, tuple_ctor_arity_from_name,
    tuple_tycon_arity_from_name,
};
use solveig_base::{
    FileId, FileSpan, FxHashMap, FxHashSet, PositionEncoding, Session, SourceFile, SourceMap,
    SourcePosition, Span, Symbol, TypeError,
};
use solveig_check::inference::holes::HoleInfo;
use solveig_check::inference::type_infer::{CheckedModule, TcMetadata};
use solveig_driver::{Compilation, CompilationSnapshot, SourceProvider, module_name_from_file};
use solveig_package::{PackageId, PackageKind};
use solveig_resolve::project::{
    EffectiveImport, ModuleApi, ModuleFile, ParsedProject, Project, ResolveError, ResolveNameKind,
    SharedModuleApi, SharedTypedModuleApi, TypeAlias, TypedModuleApi, apply_module_visibility,
    build_module_api, effective_imports, function_equation_group_indices,
    instance_method_equation_group_indices,
};
use solveig_syntax::ast::{
    ClassMethod, DocComment, ImportDecl, ImportSpec, InstanceMethod, ModuleName, ParsedModule,
    Spanned, SurfaceConstructor, SurfaceDefinition, SurfaceDoStmt, SurfaceExpr, SurfacePattern,
    SurfacePredicate, SurfaceTy,
};
use solveig_syntax::syntax::SyntaxKind;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};

mod code_actions;
mod completion;
mod document_highlights;
mod document_links;
mod editor_api;
mod formatting;
mod hover;
mod hover_artifacts;
mod inlay_hints;
mod navigation;
mod position;
mod rename;
mod semantic_tokens;
mod signature_help;
mod source_code_action;
mod source_completion;
mod source_document_highlight;
mod source_document_link;
mod source_edit;
mod source_folding_range;
mod source_inlay_hint;
mod source_lookup;
mod source_selection_range;
mod source_semantic_token;
mod source_signature;
mod source_signature_help;
mod source_symbol;
mod symbol_artifacts;
mod syntax_features;

use code_actions::{
    code_actions_in_compilation, hole_code_actions, inferred_signature_code_actions,
    resolve_code_actions, type_error_code_actions,
};
use completion::{completion_items_in_compilation, syntax_completion_candidates};
use document_highlights::document_highlights_at_position_in_artifacts;
use document_links::document_links_in_artifacts;
pub use editor_api::{CompilationIdeExt, ProjectSnapshotView};
use formatting::{format_document_in_artifacts, format_range_in_artifacts};
use hover::{hole_hover_infos, hover_at_position_in_artifacts, hover_in_artifacts};
use hover_artifacts::collect_retained_ide_artifacts;
use inlay_hints::{inlay_hints_in_compilation, project_inlay_hints, project_local_hovers};
use navigation::{
    definition_at_position_in_artifacts, definition_in_artifacts, document_symbols_in_artifacts,
    references_at_position_in_artifacts, references_in_artifacts, workspace_symbols_in_artifacts,
};
use position::{
    PositionIndex, project_position_indexes, selection_ranges_at_positions_in_artifacts,
};
use rename::{prepare_rename_in_artifacts, rename_edits_in_artifacts};
use semantic_tokens::{semantic_tokens_in_compilation, syntax_semantic_tokens};
use signature_help::signature_help_at_position_in_artifacts;
pub use source_code_action::{SourceCodeAction, SourceCodeActionCategory, SourceTextEdit};
use source_completion::SourceCompletionCandidate;
pub use source_completion::{SourceCompletionItem, SourceCompletionItemKind};
pub use source_document_highlight::{SourceDocumentHighlight, SourceDocumentHighlightKind};
pub use source_document_link::SourceDocumentLink;
pub use source_edit::SourceEditError;
pub use source_folding_range::{SourceFoldingRange, SourceFoldingRangeKind};
pub use source_inlay_hint::{
    SourceInlayHint, SourceInlayHintCategory, SourceInlayHintConfig, SourceInlayHintKind,
};
pub use source_selection_range::SourceSelectionRange;
pub use source_semantic_token::{
    SourceSemanticToken, SourceSemanticTokenKind, SourceSemanticTokenModifier,
    SourceSemanticTokenModifiers,
};
pub use source_signature_help::{SourceSignatureHelp, SourceSignatureInformation};
pub use source_symbol::{SourceSymbol, SourceSymbolKind};
use syntax_features::{SourceSyntaxTree, folding_ranges_in_syntax_trees, project_syntax_trees};

/// Hover content and source range before conversion to any editor protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoverInfo {
    file_span: FileSpan,
    signature: String,
    documentation: Option<String>,
    provenance: Option<String>,
    markdown: Option<String>,
}

impl HoverInfo {
    pub(crate) fn new(
        file: FileId,
        span: Range<usize>,
        signature: impl Into<String>,
        documentation: Option<String>,
    ) -> Self {
        Self {
            file_span: FileSpan::new(file, Span::from_usize_range(span)),
            signature: signature.into(),
            documentation,
            provenance: None,
            markdown: None,
        }
    }

    pub(crate) fn with_optional_provenance(mut self, provenance: Option<String>) -> Self {
        self.provenance = provenance;
        self
    }

    pub(crate) fn with_markdown(mut self, markdown: impl Into<String>) -> Self {
        self.markdown = Some(markdown.into());
        self
    }

    /// Returns the source span covered by this hover.
    pub const fn file_span(&self) -> FileSpan {
        self.file_span
    }

    /// Returns the byte span covered by this hover without the file id.
    pub fn span(&self) -> Range<usize> {
        self.file_span.span.to_usize_range()
    }

    /// Returns the primary hover text, usually a type, kind, signature, or diagnostic headline.
    pub fn signature(&self) -> &str {
        &self.signature
    }

    /// Returns documentation attached to the hovered symbol, when available.
    pub fn documentation(&self) -> Option<&str> {
        self.documentation.as_deref()
    }

    /// Returns the defining module or source provenance, when available.
    pub fn provenance(&self) -> Option<&str> {
        self.provenance.as_deref()
    }

    /// Returns richer Markdown content for hovers that need custom formatting.
    pub fn markdown(&self) -> Option<&str> {
        self.markdown.as_deref()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CompilationArtifacts {
    pub(crate) names: Names,
    pub(crate) checked: FxHashMap<ModuleId, CheckedModule>,
    pub(crate) module_files: FxHashMap<ModuleId, FileId>,
    pub(crate) source_files: Vec<SourceFile>,
    pub(crate) docs: FxHashMap<DefId, DocComment>,
    pub(crate) source_signatures: FxHashMap<DefId, String>,
    pub(crate) type_aliases: FxHashMap<DefId, (String, TypeAlias)>,
    pub(crate) resolutions: FxHashMap<ModuleId, Vec<(FileSpan, DefId)>>,
    pub(crate) declaration_spans: Vec<(FileSpan, DefId)>,
    pub(crate) module_display_names: FxHashMap<ModuleId, String>,
    pub(crate) module_package_kinds: FxHashMap<ModuleId, PackageKind>,
    pub(crate) module_paths: FxHashMap<ModuleId, PathBuf>,
    pub(crate) module_imports: Vec<(FileSpan, ModuleId)>,
    pub(crate) local_hovers: Vec<HoverInfo>,
    pub(crate) source_symbols: Vec<SourceSymbol>,
    pub(crate) position_indexes: FxHashMap<FileId, PositionIndex>,
    pub(crate) tycon_table: TyConTable,
    pub(crate) ctor_table: CtorTable,
    pub(crate) class_table: ClassTable,
}

#[derive(Clone, Debug)]
struct IdeSnapshotCache {
    fingerprint: u64,
    artifacts: Option<CompilationArtifacts>,
    completion_candidates: Option<Vec<SourceCompletionCandidate>>,
    inlay_hints: Option<Vec<SourceInlayHint>>,
    syntax_trees: Option<Vec<SourceSyntaxTree>>,
    syntax_tokens: Option<Vec<SourceSemanticToken>>,
}

impl IdeSnapshotCache {
    fn new(fingerprint: u64) -> Self {
        Self {
            fingerprint,
            artifacts: None,
            completion_candidates: None,
            inlay_hints: None,
            syntax_trees: None,
            syntax_tokens: None,
        }
    }
}

thread_local! {
    static IDE_SNAPSHOT_CACHE: RefCell<Option<IdeSnapshotCache>> = const { RefCell::new(None) };
}

pub(crate) fn with_artifacts_for_snapshot<T>(
    session: &Session,
    snapshot: &CompilationSnapshot,
    f: impl FnOnce(&CompilationArtifacts) -> T,
) -> T {
    with_snapshot_cache(session, snapshot, |cache| {
        if cache.artifacts.is_none() {
            cache.artifacts = Some(build_artifacts_for_snapshot(session, snapshot));
        }

        f(cache
            .artifacts
            .as_ref()
            .expect("IDE artifacts should be cached"))
    })
}

fn build_artifacts_for_snapshot(
    session: &Session,
    snapshot: &CompilationSnapshot,
) -> CompilationArtifacts {
    let retained = collect_retained_ide_artifacts(
        session,
        &snapshot.names,
        &snapshot.project,
        &snapshot.typed_apis,
    );
    let order = snapshot_module_names(session, snapshot);
    let (tycon_table, ctor_table, class_table) = checked_project_tables(&snapshot.modules);
    let mut local_hovers = project_local_hovers(
        session,
        &snapshot.project,
        &snapshot.parsed,
        &order,
        &snapshot.checked,
        &snapshot.names,
        &snapshot.modules,
    );
    let constructor_types = constructor_types_in_modules(&snapshot.modules);
    local_hovers.extend(hole_hover_infos(
        &snapshot.holes,
        &constructor_types,
        &ctor_table,
        PrettyTables::new(&snapshot.names, &tycon_table, &class_table),
    ));
    local_hovers.extend(retained.source_type_var_hovers.iter().cloned());
    local_hovers.sort_by_key(|info| {
        let span = info.file_span();
        (span.file, span.span.start, span.span.end)
    });

    let mut resolutions = snapshot.resolutions.clone();
    for (module_id, span, def_id) in &retained.import_name_resolutions {
        resolutions
            .entry(*module_id)
            .or_default()
            .push((*span, *def_id));
    }
    sort_and_dedup_resolutions(&mut resolutions);
    let position_indexes = project_position_indexes(
        session,
        snapshot,
        &retained,
        &resolutions,
        &local_hovers,
        &ctor_table,
        PrettyTables::new(&snapshot.names, &tycon_table, &class_table),
    );

    CompilationArtifacts {
        names: snapshot.names.clone(),
        checked: snapshot.checked.clone(),
        module_files: snapshot.module_files.clone(),
        source_files: snapshot.source_files.clone(),
        docs: retained.docs,
        source_signatures: retained.source_signatures,
        type_aliases: retained.type_aliases,
        resolutions,
        declaration_spans: retained.declaration_spans,
        module_display_names: retained.module_display_names,
        module_package_kinds: retained.module_package_kinds,
        module_paths: retained.module_paths,
        module_imports: retained.module_imports,
        local_hovers,
        source_symbols: retained.source_symbols,
        position_indexes,
        tycon_table,
        ctor_table,
        class_table,
    }
}

pub(crate) fn source_code_actions_for_snapshot(
    session: &Session,
    source_map: &SourceMap,
    source_provider: &SourceProvider,
    snapshot: &CompilationSnapshot,
) -> Vec<SourceCodeAction> {
    let mut out = Vec::new();

    for group in &snapshot.resolve_errors {
        out.extend(resolve_code_actions(
            session,
            &snapshot.source_root,
            source_provider,
            &group.module,
            &snapshot.project,
            &snapshot.apis,
            &group.errors,
        ));
    }

    out.extend(type_error_code_actions(source_map, &snapshot.type_errors));

    let (tycon_table, ctor_table, class_table) = checked_project_tables(&snapshot.modules);
    let constructor_types = constructor_types_in_modules(&snapshot.modules);
    out.extend(hole_code_actions(
        &snapshot.holes,
        &constructor_types,
        &ctor_table,
        PrettyTables::new(&snapshot.names, &tycon_table, &class_table),
    ));

    let order = snapshot_module_names(session, snapshot);
    for module_name in &order {
        let Some(module_id) = snapshot.project.module_table.id(module_name) else {
            continue;
        };
        let Some(checked) = snapshot.checked.get(&module_id) else {
            continue;
        };
        let parsed = snapshot.parsed.get(session, module_name);
        out.extend(inferred_signature_code_actions(
            session,
            parsed,
            module_id,
            checked,
            PrettyTables::new(&snapshot.names, &tycon_table, &class_table),
        ));
    }

    out.retain(|action| {
        source_edit::code_action_is_editable(&snapshot.project, source_map, action)
    });

    out
}

pub(crate) fn with_artifacts_and_completion_candidates_for_snapshot<T>(
    session: &Session,
    snapshot: &CompilationSnapshot,
    f: impl FnOnce(&CompilationArtifacts, &[SourceCompletionCandidate]) -> T,
) -> T {
    with_snapshot_cache(session, snapshot, |cache| {
        if cache.artifacts.is_none() {
            cache.artifacts = Some(build_artifacts_for_snapshot(session, snapshot));
        }
        if cache.completion_candidates.is_none() {
            cache.completion_candidates =
                Some(build_completion_candidates_for_snapshot(session, snapshot));
        }

        f(
            cache
                .artifacts
                .as_ref()
                .expect("IDE artifacts should be cached"),
            cache
                .completion_candidates
                .as_ref()
                .expect("IDE completion candidates should be cached"),
        )
    })
}

fn build_completion_candidates_for_snapshot(
    session: &Session,
    snapshot: &CompilationSnapshot,
) -> Vec<SourceCompletionCandidate> {
    let mut candidates = syntax_completion_candidates(session, &snapshot.project);
    let order = snapshot_module_names(session, snapshot);
    let (tycon_table, _ctor_table, class_table) = checked_project_tables(&snapshot.modules);
    let tables = PrettyTables::new(&snapshot.names, &tycon_table, &class_table);

    for module_name in &order {
        let Some(module_id) = snapshot.project.module_table.id(module_name) else {
            continue;
        };
        let Some(checked) = snapshot.checked.get(&module_id) else {
            continue;
        };
        let parsed = snapshot.parsed.get(session, module_name);
        candidates.extend(completion::typed_local_completion_candidates(
            checked,
            parsed.file,
            tables,
        ));

        let Some(local_api) = snapshot.apis.get(module_name) else {
            continue;
        };
        let Some(local_typed_api) = snapshot.typed_apis.get(module_name) else {
            continue;
        };
        let imports =
            effective_imports(&snapshot.project, module_name, &parsed.imports, parsed.file);
        candidates.extend(completion::typed_module_completion_candidates(
            completion::TypedModuleCompletionInput {
                session,
                names: &snapshot.names,
                module_name,
                parsed,
                imports: &imports,
                local_api,
                local_typed_api,
                typed_apis: &snapshot.typed_apis,
            },
        ));
    }

    candidates
}

pub(crate) fn with_inlay_hints_for_snapshot<T>(
    session: &Session,
    snapshot: &CompilationSnapshot,
    f: impl FnOnce(&[SourceInlayHint]) -> T,
) -> T {
    with_snapshot_cache(session, snapshot, |cache| {
        if cache.inlay_hints.is_none() {
            cache.inlay_hints = Some(build_inlay_hints_for_snapshot(session, snapshot));
        }

        f(cache
            .inlay_hints
            .as_ref()
            .expect("IDE inlay hints should be cached"))
    })
}

fn build_inlay_hints_for_snapshot(
    session: &Session,
    snapshot: &CompilationSnapshot,
) -> Vec<SourceInlayHint> {
    let order = snapshot_module_names(session, snapshot);
    project_inlay_hints(
        session,
        &snapshot.project,
        &snapshot.parsed,
        &order,
        &snapshot.checked,
        &snapshot.names,
        &snapshot.modules,
    )
}

pub(crate) fn with_syntax_trees_for_snapshot<T>(
    session: &Session,
    snapshot: &CompilationSnapshot,
    f: impl FnOnce(&[SourceSyntaxTree]) -> T,
) -> T {
    with_snapshot_cache(session, snapshot, |cache| {
        if cache.syntax_trees.is_none() {
            cache.syntax_trees = Some(project_syntax_trees(&snapshot.project));
        }

        f(cache
            .syntax_trees
            .as_ref()
            .expect("IDE syntax trees should be cached"))
    })
}

pub(crate) fn with_artifacts_and_syntax_tokens_for_snapshot<T>(
    session: &Session,
    snapshot: &CompilationSnapshot,
    f: impl FnOnce(&CompilationArtifacts, &[SourceSemanticToken]) -> T,
) -> T {
    with_snapshot_cache(session, snapshot, |cache| {
        if cache.artifacts.is_none() {
            cache.artifacts = Some(build_artifacts_for_snapshot(session, snapshot));
        }
        if cache.syntax_tokens.is_none() {
            cache.syntax_tokens = Some(syntax_semantic_tokens(&snapshot.project));
        }

        f(
            cache
                .artifacts
                .as_ref()
                .expect("IDE artifacts should be cached"),
            cache
                .syntax_tokens
                .as_ref()
                .expect("IDE syntax tokens should be cached"),
        )
    })
}

fn with_snapshot_cache<T>(
    session: &Session,
    snapshot: &CompilationSnapshot,
    f: impl FnOnce(&mut IdeSnapshotCache) -> T,
) -> T {
    let fingerprint = snapshot_fingerprint(session, snapshot);
    IDE_SNAPSHOT_CACHE.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot
            .as_ref()
            .is_none_or(|cache| cache.fingerprint != fingerprint)
        {
            *slot = Some(IdeSnapshotCache::new(fingerprint));
        }

        f(slot
            .as_mut()
            .expect("IDE snapshot cache should be initialized"))
    })
}

fn snapshot_fingerprint(session: &Session, snapshot: &CompilationSnapshot) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_snapshot_core(snapshot, &mut hasher);
    for module_name in snapshot_module_names(session, snapshot) {
        module_name.as_string(session).hash(&mut hasher);
        if let Some(module_file) = snapshot.project.modules.get(&module_name) {
            hash_module_file(module_file, &mut hasher);
        }
    }
    hasher.finish()
}

fn hash_snapshot_core(snapshot: &CompilationSnapshot, hasher: &mut DefaultHasher) {
    snapshot.source_root.hash(hasher);
    snapshot.project.modules.len().hash(hasher);
    snapshot.modules.len().hash(hasher);
    snapshot.checked.len().hash(hasher);
    snapshot.resolve_errors.len().hash(hasher);
    snapshot.type_errors.len().hash(hasher);
    snapshot.holes.len().hash(hasher);
}

fn hash_module_file(module_file: &ModuleFile, hasher: &mut DefaultHasher) {
    module_file.package.hash(hasher);
    module_file.path.hash(hasher);
    module_file.source_hash.hash(hasher);
    module_file.parsed.file.0.hash(hasher);
}

fn snapshot_module_names(session: &Session, snapshot: &CompilationSnapshot) -> Vec<ModuleName> {
    let mut modules = snapshot.project.modules.keys().cloned().collect::<Vec<_>>();
    modules.sort_by_key(|module| module.as_string(session));
    modules
}

fn checked_project_tables(modules: &[CheckedModule]) -> (TyConTable, CtorTable, ClassTable) {
    let mut tycon_table = TyConTable::new();
    let mut ctor_table = CtorTable::new();
    let mut class_table = ClassTable::new();

    for module in modules {
        for (id, info) in module.tycon_table.iter() {
            tycon_table.insert_with_id(id, info.clone());
        }

        for (id, info) in module.ctor_table.iter() {
            ctor_table.insert_with_id(id, info.clone());
        }

        for (id, info) in module.class_table.iter() {
            class_table.insert_with_id(id, info.clone());
        }
    }

    (tycon_table, ctor_table, class_table)
}

fn constructor_types_in_modules(modules: &[CheckedModule]) -> FxHashMap<CtorId, Ty> {
    let mut constructor_types = FxHashMap::default();

    for module in modules {
        constructor_types.extend(module.constructor_types.clone());
    }

    constructor_types
}

fn sort_and_dedup_resolutions(resolutions: &mut FxHashMap<ModuleId, Vec<(FileSpan, DefId)>>) {
    for entries in resolutions.values_mut() {
        entries.sort_by_key(|(span, def)| {
            (
                span.file,
                span.span.start,
                span.span.end,
                def.module,
                def.name,
            )
        });
        entries.dedup();
    }
}
