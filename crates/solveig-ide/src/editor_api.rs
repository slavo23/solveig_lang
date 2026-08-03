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
    ) -> Option<FileSpan>;

    /// Checks the current project and returns edits that rename a semantic symbol.
    fn rename_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
        new_name: &str,
    ) -> Vec<SourceTextEdit>;

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

impl CompilationIdeExt for Compilation {
    fn hover(&self, file: impl AsRef<Path>, offset: usize) -> Option<String> {
        self.hover_info(file, offset)
            .map(|info| info.signature().to_string())
    }

    fn hover_info(&self, file: impl AsRef<Path>, offset: usize) -> Option<HoverInfo> {
        with_artifacts_for_snapshot(self.session(), self.snapshot()?, |artifacts| {
            hover_in_artifacts(artifacts, file.as_ref(), offset)
        })
    }

    fn hover_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<HoverInfo> {
        self.ensure_checked(true).ok()?;
        with_artifacts_for_snapshot(self.session(), self.snapshot()?, |artifacts| {
            hover_at_position_in_artifacts(
                artifacts,
                self.source_map(),
                file.as_ref(),
                position,
                encoding,
            )
        })
    }

    fn definition(&self, file: impl AsRef<Path>, offset: usize) -> Option<FileSpan> {
        with_artifacts_for_snapshot(self.session(), self.snapshot()?, |artifacts| {
            definition_in_artifacts(artifacts, file.as_ref(), offset)
        })
    }

    fn definition_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<FileSpan> {
        self.ensure_checked(true).ok()?;
        with_artifacts_for_snapshot(self.session(), self.snapshot()?, |artifacts| {
            definition_at_position_in_artifacts(
                artifacts,
                self.source_map(),
                file.as_ref(),
                position,
                encoding,
            )
        })
    }

    fn references(
        &self,
        file: impl AsRef<Path>,
        offset: usize,
        include_declaration: bool,
    ) -> Option<Vec<FileSpan>> {
        with_artifacts_for_snapshot(self.session(), self.snapshot()?, |artifacts| {
            references_in_artifacts(artifacts, file.as_ref(), offset, include_declaration)
        })
    }

    fn references_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
        include_declaration: bool,
    ) -> Option<Vec<FileSpan>> {
        self.ensure_checked(true).ok()?;
        with_artifacts_for_snapshot(self.session(), self.snapshot()?, |artifacts| {
            references_at_position_in_artifacts(
                artifacts,
                self.source_map(),
                file.as_ref(),
                position,
                encoding,
                include_declaration,
            )
        })
    }

    fn document_symbols(&mut self, file: impl AsRef<Path>) -> Vec<SourceSymbol> {
        if !self.ensure_checked(true).is_ok_and(CheckResult::success) {
            return Vec::new();
        }

        let Some(snapshot) = self.snapshot() else {
            return Vec::new();
        };
        with_artifacts_for_snapshot(self.session(), snapshot, |artifacts| {
            document_symbols_in_artifacts(artifacts, file.as_ref())
        })
    }

    fn workspace_symbols(&mut self, query: &str) -> Vec<SourceSymbol> {
        if !self.ensure_checked(true).is_ok_and(CheckResult::success) {
            return Vec::new();
        }

        let Some(snapshot) = self.snapshot() else {
            return Vec::new();
        };
        with_artifacts_for_snapshot(self.session(), snapshot, |artifacts| {
            workspace_symbols_in_artifacts(artifacts, query)
        })
    }

    fn semantic_tokens(&mut self, file: impl AsRef<Path>) -> Vec<SourceSemanticToken> {
        let _ = self.ensure_checked(true);
        let Some(snapshot) = self.snapshot() else {
            return Vec::new();
        };
        with_artifacts_and_syntax_tokens_for_snapshot(
            self.session(),
            snapshot,
            |artifacts, syntax_tokens| {
                semantic_tokens_in_compilation(
                    Some(artifacts),
                    self.source_map(),
                    syntax_tokens,
                    file.as_ref(),
                )
            },
        )
    }

    fn code_actions_at_range(
        &mut self,
        file: impl AsRef<Path>,
        start: SourcePosition,
        end: SourcePosition,
        encoding: PositionEncoding,
    ) -> Vec<SourceCodeAction> {
        if self.source_map().is_empty() {
            let _ = self.ensure_checked(true);
        }

        let Some(file_id) =
            source_lookup::source_file_id_in_source_map(self.source_map(), file.as_ref())
        else {
            return Vec::new();
        };
        let start = self.source_map().offset(file_id, start, encoding);
        let end = self.source_map().offset(file_id, end, encoding);
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };

        let Some(snapshot) = self.snapshot() else {
            return Vec::new();
        };
        let actions = source_code_actions_for_snapshot(
            self.session(),
            self.source_map(),
            self.source_provider(),
            snapshot,
        );

        code_actions_in_compilation(
            self.source_map(),
            &actions,
            file.as_ref(),
            Span::new(start, end),
        )
    }

    fn source_code_actions(&self) -> Vec<SourceCodeAction> {
        let Some(snapshot) = self.snapshot() else {
            return Vec::new();
        };

        source_code_actions_for_snapshot(
            self.session(),
            self.source_map(),
            self.source_provider(),
            snapshot,
        )
    }

    fn completion_items_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Vec<SourceCompletionItem> {
        if self.source_map().is_empty() {
            let _ = self.ensure_checked(true);
        }

        let Some(snapshot) = self.snapshot() else {
            return Vec::new();
        };
        with_artifacts_and_completion_candidates_for_snapshot(
            self.session(),
            snapshot,
            |artifacts, candidates| {
                completion_items_in_compilation(
                    artifacts,
                    self.source_map(),
                    candidates,
                    file.as_ref(),
                    position,
                    encoding,
                )
            },
        )
    }

    fn document_highlights_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Vec<SourceDocumentHighlight> {
        if !self.ensure_checked(true).is_ok_and(CheckResult::success) {
            return Vec::new();
        }

        let Some(snapshot) = self.snapshot() else {
            return Vec::new();
        };
        with_artifacts_for_snapshot(self.session(), snapshot, |artifacts| {
            document_highlights_at_position_in_artifacts(
                artifacts,
                self.source_map(),
                file.as_ref(),
                position,
                encoding,
            )
        })
    }

    fn folding_ranges(&mut self, file: impl AsRef<Path>) -> Vec<SourceFoldingRange> {
        let _ = self.ensure_checked(true);
        let Some(snapshot) = self.snapshot() else {
            return Vec::new();
        };
        with_syntax_trees_for_snapshot(self.session(), snapshot, |syntax_trees| {
            folding_ranges_in_syntax_trees(self.source_map(), syntax_trees, file.as_ref())
        })
    }

    fn selection_ranges(
        &mut self,
        file: impl AsRef<Path>,
        positions: &[SourcePosition],
        encoding: PositionEncoding,
    ) -> Vec<SourceSelectionRange> {
        let _ = self.ensure_checked(true);
        let Some(snapshot) = self.snapshot() else {
            return Vec::new();
        };
        with_artifacts_for_snapshot(self.session(), snapshot, |artifacts| {
            selection_ranges_at_positions_in_artifacts(
                artifacts,
                self.source_map(),
                file.as_ref(),
                positions,
                encoding,
            )
        })
    }

    fn signature_help_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<SourceSignatureHelp> {
        if !self.ensure_checked(true).is_ok_and(CheckResult::success) {
            return None;
        }

        with_artifacts_for_snapshot(self.session(), self.snapshot()?, |artifacts| {
            signature_help_at_position_in_artifacts(
                artifacts,
                self.source_map(),
                file.as_ref(),
                position,
                encoding,
            )
        })
    }

    fn prepare_rename_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<FileSpan> {
        if !self.ensure_checked(true).is_ok_and(CheckResult::success) {
            return None;
        }

        with_artifacts_for_snapshot(self.session(), self.snapshot()?, |artifacts| {
            prepare_rename_in_artifacts(
                artifacts,
                self.source_map(),
                file.as_ref(),
                position,
                encoding,
            )
        })
    }

    fn rename_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
        new_name: &str,
    ) -> Vec<SourceTextEdit> {
        if !self.ensure_checked(true).is_ok_and(CheckResult::success) {
            return Vec::new();
        }

        let Some(snapshot) = self.snapshot() else {
            return Vec::new();
        };
        with_artifacts_for_snapshot(self.session(), snapshot, |artifacts| {
            rename_edits_in_artifacts(
                artifacts,
                self.source_map(),
                file.as_ref(),
                position,
                encoding,
                new_name,
            )
        })
    }

    fn document_links(&mut self, file: impl AsRef<Path>) -> Vec<SourceDocumentLink> {
        if !self.ensure_checked(true).is_ok_and(CheckResult::success) {
            return Vec::new();
        }

        let Some(snapshot) = self.snapshot() else {
            return Vec::new();
        };
        with_artifacts_for_snapshot(self.session(), snapshot, |artifacts| {
            document_links_in_artifacts(artifacts, self.source_map(), file.as_ref())
        })
    }

    fn inlay_hints_at_range(
        &mut self,
        file: impl AsRef<Path>,
        start: SourcePosition,
        end: SourcePosition,
        encoding: PositionEncoding,
        config: SourceInlayHintConfig,
    ) -> Vec<SourceInlayHint> {
        let _ = self.ensure_checked(true);
        let Some(snapshot) = self.snapshot() else {
            return Vec::new();
        };
        with_inlay_hints_for_snapshot(self.session(), snapshot, |hints| {
            inlay_hints_in_compilation(
                self.source_map(),
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
