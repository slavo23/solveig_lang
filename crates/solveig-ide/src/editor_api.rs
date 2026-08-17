//! Public editor-facing query methods over retained compilation snapshots.

use super::*;
use solveig_driver::CheckResult;

/// Adds editor queries to `solveig_driver::Compilation`.
pub trait CompilationIdeExt {
    /// Looks up hover text at a byte offset after a prior successful check/run.
    fn hover(&self, file: impl AsRef<Path>, offset: usize) -> Option<String>;

    /// Looks up semantic hover information at a byte offset after a prior successful check/run.
    fn hover_info(&self, file: impl AsRef<Path>, offset: usize) -> Option<HoverInfo>;

    /// Checks the current project and looks up semantic hover information at an editor position.
    fn hover_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<HoverInfo>;

    /// Looks up a definition target at a byte offset after a prior successful check/run.
    fn definition(&self, file: impl AsRef<Path>, offset: usize) -> Option<FileSpan>;

    /// Checks the current project and looks up a definition target at an editor position.
    fn definition_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<FileSpan>;

    /// Looks up project-wide references at a byte offset after a prior successful check/run.
    fn references(
        &self,
        file: impl AsRef<Path>,
        offset: usize,
        include_declaration: bool,
    ) -> Option<Vec<FileSpan>>;

    /// Checks the current project and looks up project-wide references at an editor position.
    fn references_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
        include_declaration: bool,
    ) -> Option<Vec<FileSpan>>;

    /// Checks the current project and returns hierarchical symbols for one source file.
    fn document_symbols(&mut self, file: impl AsRef<Path>) -> Vec<SourceSymbol>;

    /// Checks the current project and returns project-wide symbols matching `query`.
    fn workspace_symbols(&mut self, query: &str) -> Vec<SourceSymbol>;

    /// Checks the current project and returns semantic tokens for one source file.
    fn semantic_tokens(&mut self, file: impl AsRef<Path>) -> Vec<SourceSemanticToken>;

    /// Checks whether mutating editor features may edit a source file.
    fn ensure_source_editable(&mut self, file: impl AsRef<Path>) -> Result<(), SourceEditError>;

    /// Checks the current project and returns code actions visible for an editor range.
    fn code_actions_at_range(
        &mut self,
        file: impl AsRef<Path>,
        start: SourcePosition,
        end: SourcePosition,
        encoding: PositionEncoding,
    ) -> Vec<SourceCodeAction>;

    /// Returns all retained source-level code actions after a prior check.
    fn source_code_actions(&self) -> Vec<SourceCodeAction>;

    /// Checks the current project and returns completion items visible at an editor position.
    fn completion_items_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Vec<SourceCompletionItem>;

    /// Checks the current project and returns document highlights for the symbol at a position.
    fn document_highlights_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Vec<SourceDocumentHighlight>;

    /// Checks the current project and returns foldable ranges for one source file.
    fn folding_ranges(&mut self, file: impl AsRef<Path>) -> Vec<SourceFoldingRange>;

    /// Checks the current project and returns CST-backed selection ranges.
    fn selection_ranges(
        &mut self,
        file: impl AsRef<Path>,
        positions: &[SourcePosition],
        encoding: PositionEncoding,
    ) -> Vec<SourceSelectionRange>;

    /// Checks the current project and returns signature help at an editor position.
    fn signature_help_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<SourceSignatureHelp>;

    /// Checks whether the symbol at an editor position can be renamed.
    fn prepare_rename_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Result<Option<FileSpan>, SourceEditError>;

    /// Checks the current project and returns edits that rename a semantic symbol.
    fn rename_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
        new_name: &str,
    ) -> Result<Vec<SourceTextEdit>, SourceEditError>;

    /// Checks the current project and returns module document links for one source file.
    fn document_links(&mut self, file: impl AsRef<Path>) -> Vec<SourceDocumentLink>;

    /// Checks the current project and returns inlay hints visible for an editor range.
    fn inlay_hints_at_range(
        &mut self,
        file: impl AsRef<Path>,
        start: SourcePosition,
        end: SourcePosition,
        encoding: PositionEncoding,
        config: SourceInlayHintConfig,
    ) -> Vec<SourceInlayHint>;
}

/// Provides editor queries over one retained driver snapshot without re-checking.
pub struct ProjectSnapshotView<'a> {
    session: &'a Session,
    source_map: &'a SourceMap,
    source_provider: &'a SourceProvider,
    snapshot: &'a CompilationSnapshot,
    success: bool,
}

impl<'a> ProjectSnapshotView<'a> {
    /// Creates a borrowed editor-query view over a checked project snapshot.
    pub fn new(
        session: &'a Session,
        source_map: &'a SourceMap,
        source_provider: &'a SourceProvider,
        snapshot: &'a CompilationSnapshot,
        success: bool,
    ) -> Self {
        Self {
            session,
            source_map,
            source_provider,
            snapshot,
            success,
        }
    }

    /// Looks up semantic hover information at a byte offset.
    pub fn hover_info(&self, file: impl AsRef<Path>, offset: usize) -> Option<HoverInfo> {
        with_artifacts_for_snapshot(self.session, self.snapshot, |artifacts| {
            hover_in_artifacts(artifacts, file.as_ref(), offset)
        })
    }

    /// Looks up semantic hover information at an editor position.
    pub fn hover_at_position(
        &self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<HoverInfo> {
        with_artifacts_for_snapshot(self.session, self.snapshot, |artifacts| {
            hover_at_position_in_artifacts(
                artifacts,
                self.source_map,
                file.as_ref(),
                position,
                encoding,
            )
        })
    }

    /// Looks up a definition target at a byte offset.
    pub fn definition(&self, file: impl AsRef<Path>, offset: usize) -> Option<FileSpan> {
        with_artifacts_for_snapshot(self.session, self.snapshot, |artifacts| {
            definition_in_artifacts(artifacts, file.as_ref(), offset)
        })
    }

    /// Looks up a definition target at an editor position.
    pub fn definition_at_position(
        &self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<FileSpan> {
        with_artifacts_for_snapshot(self.session, self.snapshot, |artifacts| {
            definition_at_position_in_artifacts(
                artifacts,
                self.source_map,
                file.as_ref(),
                position,
                encoding,
            )
        })
    }

    /// Looks up project-wide references at a byte offset.
    pub fn references(
        &self,
        file: impl AsRef<Path>,
        offset: usize,
        include_declaration: bool,
    ) -> Option<Vec<FileSpan>> {
        with_artifacts_for_snapshot(self.session, self.snapshot, |artifacts| {
            references_in_artifacts(artifacts, file.as_ref(), offset, include_declaration)
        })
    }

    /// Looks up project-wide references at an editor position.
    pub fn references_at_position(
        &self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
        include_declaration: bool,
    ) -> Option<Vec<FileSpan>> {
        with_artifacts_for_snapshot(self.session, self.snapshot, |artifacts| {
            references_at_position_in_artifacts(
                artifacts,
                self.source_map,
                file.as_ref(),
                position,
                encoding,
                include_declaration,
            )
        })
    }

    /// Returns hierarchical symbols for one source file.
    pub fn document_symbols(&self, file: impl AsRef<Path>) -> Vec<SourceSymbol> {
        if !self.success {
            return Vec::new();
        }

        with_artifacts_for_snapshot(self.session, self.snapshot, |artifacts| {
            document_symbols_in_artifacts(artifacts, file.as_ref())
        })
    }

    /// Returns project-wide symbols matching `query`.
    pub fn workspace_symbols(&self, query: &str) -> Vec<SourceSymbol> {
        if !self.success {
            return Vec::new();
        }

        with_artifacts_for_snapshot(self.session, self.snapshot, |artifacts| {
            workspace_symbols_in_artifacts(artifacts, query)
        })
    }

    /// Returns semantic tokens for one source file.
    pub fn semantic_tokens(&self, file: impl AsRef<Path>) -> Vec<SourceSemanticToken> {
        with_artifacts_and_syntax_tokens_for_snapshot(
            self.session,
            self.snapshot,
            |artifacts, syntax_tokens| {
                semantic_tokens_in_compilation(
                    Some(artifacts),
                    self.source_map,
                    syntax_tokens,
                    file.as_ref(),
                )
            },
        )
    }

    /// Checks whether mutating editor features, including formatting, may edit a source file.
    pub fn ensure_source_editable(&self, file: impl AsRef<Path>) -> Result<(), SourceEditError> {
        source_edit::ensure_source_editable(&self.snapshot.project, self.source_map, file.as_ref())
    }

    /// Returns all retained source-level code actions.
    pub fn source_code_actions(&self) -> Vec<SourceCodeAction> {
        source_code_actions_for_snapshot(
            self.session,
            self.source_map,
            self.source_provider,
            self.snapshot,
        )
    }

    /// Returns code actions visible for an editor range.
    pub fn code_actions_at_range(
        &self,
        file: impl AsRef<Path>,
        start: SourcePosition,
        end: SourcePosition,
        encoding: PositionEncoding,
    ) -> Vec<SourceCodeAction> {
        if self.ensure_source_editable(file.as_ref()).is_err() {
            return Vec::new();
        }
        if self.source_map.is_empty() {
            return Vec::new();
        }

        let Some(file_id) =
            source_lookup::source_file_id_in_source_map(self.source_map, file.as_ref())
        else {
            return Vec::new();
        };
        let start = self.source_map.offset(file_id, start, encoding);
        let end = self.source_map.offset(file_id, end, encoding);
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let actions = self.source_code_actions();

        code_actions_in_compilation(
            self.source_map,
            &actions,
            file.as_ref(),
            Span::new(start, end),
        )
    }

    /// Returns completion items visible at an editor position.
    pub fn completion_items_at_position(
        &self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Vec<SourceCompletionItem> {
        with_artifacts_and_completion_candidates_for_snapshot(
            self.session,
            self.snapshot,
            |artifacts, candidates| {
                completion_items_in_compilation(
                    artifacts,
                    self.source_map,
                    candidates,
                    file.as_ref(),
                    position,
                    encoding,
                )
            },
        )
    }

    /// Returns document highlights for the symbol at an editor position.
    pub fn document_highlights_at_position(
        &self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Vec<SourceDocumentHighlight> {
        if !self.success {
            return Vec::new();
        }

        with_artifacts_for_snapshot(self.session, self.snapshot, |artifacts| {
            document_highlights_at_position_in_artifacts(
                artifacts,
                self.source_map,
                file.as_ref(),
                position,
                encoding,
            )
        })
    }

    /// Returns foldable ranges for one source file.
    pub fn folding_ranges(&self, file: impl AsRef<Path>) -> Vec<SourceFoldingRange> {
        with_syntax_trees_for_snapshot(self.session, self.snapshot, |syntax_trees| {
            folding_ranges_in_syntax_trees(self.source_map, syntax_trees, file.as_ref())
        })
    }

    /// Returns CST-backed selection ranges.
    pub fn selection_ranges(
        &self,
        file: impl AsRef<Path>,
        positions: &[SourcePosition],
        encoding: PositionEncoding,
    ) -> Vec<SourceSelectionRange> {
        with_artifacts_for_snapshot(self.session, self.snapshot, |artifacts| {
            selection_ranges_at_positions_in_artifacts(
                artifacts,
                self.source_map,
                file.as_ref(),
                positions,
                encoding,
            )
        })
    }

    /// Returns signature help at an editor position.
    pub fn signature_help_at_position(
        &self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<SourceSignatureHelp> {
        if !self.success {
            return None;
        }

        with_artifacts_for_snapshot(self.session, self.snapshot, |artifacts| {
            signature_help_at_position_in_artifacts(
                artifacts,
                self.source_map,
                file.as_ref(),
                position,
                encoding,
            )
        })
    }

    /// Checks whether the symbol at an editor position can be renamed.
    pub fn prepare_rename_at_position(
        &self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Result<Option<FileSpan>, SourceEditError> {
        if !self.success {
            return Ok(None);
        }

        with_artifacts_for_snapshot(self.session, self.snapshot, |artifacts| {
            prepare_rename_in_artifacts(
                artifacts,
                &self.snapshot.project,
                self.source_map,
                file.as_ref(),
                position,
                encoding,
            )
        })
    }

    /// Returns edits that rename a semantic symbol.
    pub fn rename_at_position(
        &self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
        new_name: &str,
    ) -> Result<Vec<SourceTextEdit>, SourceEditError> {
        if !self.success {
            return Ok(Vec::new());
        }

        with_artifacts_for_snapshot(self.session, self.snapshot, |artifacts| {
            rename_edits_in_artifacts(
                artifacts,
                &self.snapshot.project,
                self.source_map,
                file.as_ref(),
                position,
                encoding,
                new_name,
            )
        })
    }

    /// Returns module document links for one source file.
    pub fn document_links(&self, file: impl AsRef<Path>) -> Vec<SourceDocumentLink> {
        if !self.success {
            return Vec::new();
        }

        with_artifacts_for_snapshot(self.session, self.snapshot, |artifacts| {
            document_links_in_artifacts(artifacts, self.source_map, file.as_ref())
        })
    }

    /// Returns inlay hints visible for an editor range.
    pub fn inlay_hints_at_range(
        &self,
        file: impl AsRef<Path>,
        start: SourcePosition,
        end: SourcePosition,
        encoding: PositionEncoding,
        config: SourceInlayHintConfig,
    ) -> Vec<SourceInlayHint> {
        with_inlay_hints_for_snapshot(self.session, self.snapshot, |hints| {
            inlay_hints_in_compilation(
                self.source_map,
                hints,
                file.as_ref(),
                start,
                end,
                encoding,
                config,
            )
        })
    }
}

impl CompilationIdeExt for Compilation {
    fn hover(&self, file: impl AsRef<Path>, offset: usize) -> Option<String> {
        self.hover_info(file, offset)
            .map(|info| info.signature().to_string())
    }

    fn hover_info(&self, file: impl AsRef<Path>, offset: usize) -> Option<HoverInfo> {
        project_snapshot_view(self, true)?.hover_info(file, offset)
    }

    fn hover_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<HoverInfo> {
        let success = self.ensure_checked(false).ok()?.success();

        project_snapshot_view(self, success)?.hover_at_position(file, position, encoding)
    }

    fn definition(&self, file: impl AsRef<Path>, offset: usize) -> Option<FileSpan> {
        project_snapshot_view(self, true)?.definition(file, offset)
    }

    fn definition_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<FileSpan> {
        let success = self.ensure_checked(false).ok()?.success();

        project_snapshot_view(self, success)?.definition_at_position(file, position, encoding)
    }

    fn references(
        &self,
        file: impl AsRef<Path>,
        offset: usize,
        include_declaration: bool,
    ) -> Option<Vec<FileSpan>> {
        project_snapshot_view(self, true)?.references(file, offset, include_declaration)
    }

    fn references_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
        include_declaration: bool,
    ) -> Option<Vec<FileSpan>> {
        let success = self.ensure_checked(false).ok()?.success();

        project_snapshot_view(self, success)?.references_at_position(
            file,
            position,
            encoding,
            include_declaration,
        )
    }

    fn document_symbols(&mut self, file: impl AsRef<Path>) -> Vec<SourceSymbol> {
        let success = self.ensure_checked(false).is_ok_and(CheckResult::success);

        project_snapshot_view(self, success)
            .map_or_else(Vec::new, |view| view.document_symbols(file))
    }

    fn workspace_symbols(&mut self, query: &str) -> Vec<SourceSymbol> {
        let success = self.ensure_checked(false).is_ok_and(CheckResult::success);

        project_snapshot_view(self, success)
            .map_or_else(Vec::new, |view| view.workspace_symbols(query))
    }

    fn semantic_tokens(&mut self, file: impl AsRef<Path>) -> Vec<SourceSemanticToken> {
        let success = self.ensure_checked(false).is_ok_and(CheckResult::success);

        project_snapshot_view(self, success)
            .map_or_else(Vec::new, |view| view.semantic_tokens(file))
    }

    fn ensure_source_editable(&mut self, file: impl AsRef<Path>) -> Result<(), SourceEditError> {
        let _result = self.ensure_checked(false);
        let path = file.as_ref();
        let Some(view) = project_snapshot_view(self, true) else {
            return Err(SourceEditError::UnknownSource {
                path: path.to_path_buf(),
            });
        };

        view.ensure_source_editable(path)
    }

    fn code_actions_at_range(
        &mut self,
        file: impl AsRef<Path>,
        start: SourcePosition,
        end: SourcePosition,
        encoding: PositionEncoding,
    ) -> Vec<SourceCodeAction> {
        let success = self.ensure_checked(false).is_ok_and(CheckResult::success);

        project_snapshot_view(self, success).map_or_else(Vec::new, |view| {
            view.code_actions_at_range(file, start, end, encoding)
        })
    }

    fn source_code_actions(&self) -> Vec<SourceCodeAction> {
        project_snapshot_view(self, true).map_or_else(Vec::new, |view| view.source_code_actions())
    }

    fn completion_items_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Vec<SourceCompletionItem> {
        let success = self.ensure_checked(false).is_ok_and(CheckResult::success);

        project_snapshot_view(self, success).map_or_else(Vec::new, |view| {
            view.completion_items_at_position(file, position, encoding)
        })
    }

    fn document_highlights_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Vec<SourceDocumentHighlight> {
        let success = self.ensure_checked(false).is_ok_and(CheckResult::success);

        project_snapshot_view(self, success).map_or_else(Vec::new, |view| {
            view.document_highlights_at_position(file, position, encoding)
        })
    }

    fn folding_ranges(&mut self, file: impl AsRef<Path>) -> Vec<SourceFoldingRange> {
        let success = self.ensure_checked(false).is_ok_and(CheckResult::success);

        project_snapshot_view(self, success).map_or_else(Vec::new, |view| view.folding_ranges(file))
    }

    fn selection_ranges(
        &mut self,
        file: impl AsRef<Path>,
        positions: &[SourcePosition],
        encoding: PositionEncoding,
    ) -> Vec<SourceSelectionRange> {
        let success = self.ensure_checked(false).is_ok_and(CheckResult::success);

        project_snapshot_view(self, success).map_or_else(Vec::new, |view| {
            view.selection_ranges(file, positions, encoding)
        })
    }

    fn signature_help_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<SourceSignatureHelp> {
        let success = self.ensure_checked(false).is_ok_and(CheckResult::success);

        project_snapshot_view(self, success)?.signature_help_at_position(file, position, encoding)
    }

    fn prepare_rename_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Result<Option<FileSpan>, SourceEditError> {
        let success = self.ensure_checked(false).is_ok_and(CheckResult::success);

        let Some(view) = project_snapshot_view(self, success) else {
            return Ok(None);
        };

        view.prepare_rename_at_position(file, position, encoding)
    }

    fn rename_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
        new_name: &str,
    ) -> Result<Vec<SourceTextEdit>, SourceEditError> {
        let success = self.ensure_checked(false).is_ok_and(CheckResult::success);

        let Some(view) = project_snapshot_view(self, success) else {
            return Ok(Vec::new());
        };

        view.rename_at_position(file, position, encoding, new_name)
    }

    fn document_links(&mut self, file: impl AsRef<Path>) -> Vec<SourceDocumentLink> {
        let success = self.ensure_checked(false).is_ok_and(CheckResult::success);

        project_snapshot_view(self, success).map_or_else(Vec::new, |view| view.document_links(file))
    }

    fn inlay_hints_at_range(
        &mut self,
        file: impl AsRef<Path>,
        start: SourcePosition,
        end: SourcePosition,
        encoding: PositionEncoding,
        config: SourceInlayHintConfig,
    ) -> Vec<SourceInlayHint> {
        let success = self.ensure_checked(false).is_ok_and(CheckResult::success);

        project_snapshot_view(self, success).map_or_else(Vec::new, |view| {
            view.inlay_hints_at_range(file, start, end, encoding, config)
        })
    }
}

fn project_snapshot_view(
    compilation: &Compilation,
    success: bool,
) -> Option<ProjectSnapshotView<'_>> {
    Some(ProjectSnapshotView::new(
        compilation.session(),
        compilation.source_map(),
        compilation.source_provider(),
        compilation.snapshot()?,
        success,
    ))
}
