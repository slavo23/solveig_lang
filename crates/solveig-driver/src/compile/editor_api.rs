//! Public editor-facing query methods over retained compilation artifacts.

use super::*;

impl Compilation {
    pub fn hover(&self, file: impl AsRef<Path>, offset: usize) -> Option<String> {
        self.hover_info(file, offset)
            .map(|info| info.signature().to_string())
    }

    /// Looks up semantic hover information at a byte offset after a prior successful check/run.
    pub fn hover_info(&self, file: impl AsRef<Path>, offset: usize) -> Option<HoverInfo> {
        hover_in_artifacts(self.artifacts.as_ref()?, file.as_ref(), offset)
    }

    /// Checks the current project and looks up semantic hover information at an editor position.
    pub fn hover_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<HoverInfo> {
        self.ensure_checked(true).ok()?;
        hover_at_position_in_artifacts(
            self.artifacts.as_ref()?,
            self.source_map(),
            file.as_ref(),
            position,
            encoding,
        )
    }

    /// Looks up a definition target at a byte offset after a prior successful check/run.
    pub fn definition(&self, file: impl AsRef<Path>, offset: usize) -> Option<FileSpan> {
        definition_in_artifacts(self.artifacts.as_ref()?, file.as_ref(), offset)
    }

    /// Checks the current project and looks up a definition target at an editor position.
    pub fn definition_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<FileSpan> {
        self.ensure_checked(true).ok()?;
        definition_at_position_in_artifacts(
            self.artifacts.as_ref()?,
            self.source_map(),
            file.as_ref(),
            position,
            encoding,
        )
    }

    /// Looks up project-wide references at a byte offset after a prior successful check/run.
    pub fn references(
        &self,
        file: impl AsRef<Path>,
        offset: usize,
        include_declaration: bool,
    ) -> Option<Vec<FileSpan>> {
        references_in_artifacts(
            self.artifacts.as_ref()?,
            file.as_ref(),
            offset,
            include_declaration,
        )
    }

    /// Checks the current project and looks up project-wide references at an editor position.
    pub fn references_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
        include_declaration: bool,
    ) -> Option<Vec<FileSpan>> {
        self.ensure_checked(true).ok()?;
        references_at_position_in_artifacts(
            self.artifacts.as_ref()?,
            self.source_map(),
            file.as_ref(),
            position,
            encoding,
            include_declaration,
        )
    }

    /// Checks the current project and returns hierarchical symbols for one source file.
    pub fn document_symbols(&mut self, file: impl AsRef<Path>) -> Vec<SourceSymbol> {
        if !self.ensure_checked(true).is_ok_and(CheckResult::success) {
            return Vec::new();
        }

        let Some(artifacts) = self.artifacts.as_ref() else {
            return Vec::new();
        };

        document_symbols_in_artifacts(artifacts, file.as_ref())
    }

    /// Checks the current project and returns project-wide symbols matching `query`.
    pub fn workspace_symbols(&mut self, query: &str) -> Vec<SourceSymbol> {
        if !self.ensure_checked(true).is_ok_and(CheckResult::success) {
            return Vec::new();
        }

        let Some(artifacts) = self.artifacts.as_ref() else {
            return Vec::new();
        };

        workspace_symbols_in_artifacts(artifacts, query)
    }

    /// Checks the current project and returns semantic tokens for one source file.
    pub fn semantic_tokens(&mut self, file: impl AsRef<Path>) -> Vec<SourceSemanticToken> {
        let _ = self.ensure_checked(true);

        semantic_tokens_in_compilation(
            self.artifacts.as_ref(),
            self.source_map(),
            &self.semantic_tokens,
            file.as_ref(),
        )
    }

    /// Checks the current project and returns code actions visible for an editor range.
    pub fn code_actions_at_range(
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

        code_actions_in_compilation(
            self.source_map(),
            &self.code_actions,
            file.as_ref(),
            Span::new(start, end),
        )
    }

    /// Checks the current project and returns completion items visible at an editor position.
    pub fn completion_items_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Vec<SourceCompletionItem> {
        if self.source_map().is_empty() {
            let _ = self.ensure_checked(true);
        }

        completion_items_in_compilation(
            self.source_map(),
            &self.completion_candidates,
            file.as_ref(),
            position,
            encoding,
        )
    }

    /// Checks the current project and returns document highlights for the symbol at a position.
    pub fn document_highlights_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Vec<SourceDocumentHighlight> {
        if !self.ensure_checked(true).is_ok_and(CheckResult::success) {
            return Vec::new();
        }

        let Some(artifacts) = self.artifacts.as_ref() else {
            return Vec::new();
        };

        document_highlights_at_position_in_artifacts(
            artifacts,
            self.source_map(),
            file.as_ref(),
            position,
            encoding,
        )
    }

    /// Checks the current project and returns foldable ranges for one source file.
    pub fn folding_ranges(&mut self, file: impl AsRef<Path>) -> Vec<SourceFoldingRange> {
        let _ = self.ensure_checked(true);

        folding_ranges_in_syntax_trees(self.source_map(), &self.syntax_trees, file.as_ref())
    }

    /// Checks the current project and returns CST-backed selection ranges.
    pub fn selection_ranges(
        &mut self,
        file: impl AsRef<Path>,
        positions: &[SourcePosition],
        encoding: PositionEncoding,
    ) -> Vec<SourceSelectionRange> {
        let _ = self.ensure_checked(true);

        selection_ranges_in_syntax_trees(
            self.source_map(),
            &self.syntax_trees,
            file.as_ref(),
            positions,
            encoding,
        )
    }

    /// Checks the current project and returns signature help at an editor position.
    pub fn signature_help_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<SourceSignatureHelp> {
        if !self.ensure_checked(true).is_ok_and(CheckResult::success) {
            return None;
        }

        signature_help_at_position_in_artifacts(
            self.artifacts.as_ref()?,
            self.source_map(),
            file.as_ref(),
            position,
            encoding,
        )
    }

    /// Checks whether the symbol at an editor position can be renamed.
    pub fn prepare_rename_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> Option<FileSpan> {
        if !self.ensure_checked(true).is_ok_and(CheckResult::success) {
            return None;
        }

        prepare_rename_in_artifacts(
            self.artifacts.as_ref()?,
            self.source_map(),
            file.as_ref(),
            position,
            encoding,
        )
    }

    /// Checks the current project and returns edits that rename a semantic symbol.
    pub fn rename_at_position(
        &mut self,
        file: impl AsRef<Path>,
        position: SourcePosition,
        encoding: PositionEncoding,
        new_name: &str,
    ) -> Vec<SourceTextEdit> {
        if !self.ensure_checked(true).is_ok_and(CheckResult::success) {
            return Vec::new();
        }

        let Some(artifacts) = self.artifacts.as_ref() else {
            return Vec::new();
        };

        rename_edits_in_artifacts(
            artifacts,
            self.source_map(),
            file.as_ref(),
            position,
            encoding,
            new_name,
        )
    }

    /// Checks the current project and returns module document links for one source file.
    pub fn document_links(&mut self, file: impl AsRef<Path>) -> Vec<SourceDocumentLink> {
        if !self.ensure_checked(true).is_ok_and(CheckResult::success) {
            return Vec::new();
        }

        let Some(artifacts) = self.artifacts.as_ref() else {
            return Vec::new();
        };

        document_links_in_artifacts(artifacts, self.source_map(), file.as_ref())
    }

    /// Checks the current project and returns inlay hints visible for an editor range.
    pub fn inlay_hints_at_range(
        &mut self,
        file: impl AsRef<Path>,
        start: SourcePosition,
        end: SourcePosition,
        encoding: PositionEncoding,
        config: SourceInlayHintConfig,
    ) -> Vec<SourceInlayHint> {
        let _ = self.ensure_checked(true);

        inlay_hints_in_compilation(
            self.source_map(),
            &self.inlay_hints,
            file.as_ref(),
            start,
            end,
            encoding,
            config,
        )
    }
}
