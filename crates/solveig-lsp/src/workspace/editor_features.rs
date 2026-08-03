//! Workspace request paths for editor features beyond diagnostics/navigation.

use std::path::Path;

use async_lsp::lsp_types::{
    DocumentHighlight, DocumentLink, FoldingRange, Position, PrepareRenameResponse, SelectionRange,
    SignatureHelp, WorkspaceEdit,
};
use solveig_base::{PositionEncoding, SourcePosition};

use super::{WorkspaceSnapshot, guarded};
use crate::editor_features::{
    lsp_document_highlights, lsp_document_links, lsp_folding_ranges, lsp_prepare_rename,
    lsp_selection_ranges, lsp_signature_help, lsp_workspace_edit,
};

impl WorkspaceSnapshot {
    pub(crate) fn document_highlights(
        &self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
    ) -> Option<Vec<DocumentHighlight>> {
        guarded("document-highlight request", || {
            let highlights = self.view().document_highlights_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
            );

            Some(lsp_document_highlights(
                &highlights,
                &self.source_map,
                encoding,
            ))
        })
        .ok()
        .flatten()
    }

    pub(crate) fn folding_ranges(
        &self,
        path: &Path,
        encoding: PositionEncoding,
    ) -> Option<Vec<FoldingRange>> {
        guarded("folding-range request", || {
            let ranges = self.view().folding_ranges(path);

            Some(lsp_folding_ranges(&ranges, &self.source_map, encoding))
        })
        .ok()
        .flatten()
    }

    pub(crate) fn selection_ranges(
        &self,
        path: &Path,
        positions: Vec<Position>,
        encoding: PositionEncoding,
    ) -> Option<Vec<SelectionRange>> {
        guarded("selection-range request", || {
            let positions = positions
                .into_iter()
                .map(|position| SourcePosition::new(position.line, position.character))
                .collect::<Vec<_>>();
            let ranges = self.view().selection_ranges(path, &positions, encoding);

            Some(lsp_selection_ranges(&ranges, &self.source_map, encoding))
        })
        .ok()
        .flatten()
    }

    pub(crate) fn signature_help(
        &self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
    ) -> Option<SignatureHelp> {
        guarded("signature-help request", || {
            let help = self.view().signature_help_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
            )?;

            Some(lsp_signature_help(&help))
        })
        .ok()
        .flatten()
    }

    pub(crate) fn prepare_rename(
        &self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
    ) -> Option<PrepareRenameResponse> {
        guarded("prepare-rename request", || {
            let span = self.view().prepare_rename_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
            )?;

            lsp_prepare_rename(span, &self.source_map, encoding)
        })
        .ok()
        .flatten()
    }

    pub(crate) fn rename(
        &self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
        new_name: &str,
    ) -> Option<WorkspaceEdit> {
        guarded("rename request", || {
            let edits = self.view().rename_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
                new_name,
            );
            if edits.is_empty() {
                return None;
            }

            lsp_workspace_edit(&edits, &self.source_map, encoding)
        })
        .ok()
        .flatten()
    }

    pub(crate) fn document_links(
        &self,
        path: &Path,
        encoding: PositionEncoding,
    ) -> Option<Vec<DocumentLink>> {
        guarded("document-link request", || {
            let links = self.view().document_links(path);

            Some(lsp_document_links(&links, &self.source_map, encoding))
        })
        .ok()
        .flatten()
    }
}
