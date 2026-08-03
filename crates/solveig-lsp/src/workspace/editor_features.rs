//! Workspace request paths for editor features beyond diagnostics/navigation.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use lsp_types::{
    DocumentHighlight, DocumentLink, FoldingRange, Position, PrepareRenameResponse, SelectionRange,
    SignatureHelp, Url, WorkspaceEdit,
};
use solveig_base::{PositionEncoding, SourcePosition};
use solveig_ide::CompilationIdeExt;

use super::{ServerState, Workspace};
use crate::editor_features::{
    lsp_document_highlights, lsp_document_links, lsp_folding_ranges, lsp_prepare_rename,
    lsp_selection_ranges, lsp_signature_help, lsp_workspace_edit,
};

impl ServerState {
    pub(crate) fn document_highlights(
        &mut self,
        uri: &Url,
        position: Position,
    ) -> Option<Vec<DocumentHighlight>> {
        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;

        self.workspace_for_path(&path)
            .document_highlights(&path, position, encoding)
    }

    pub(crate) fn folding_ranges(&mut self, uri: &Url) -> Option<Vec<FoldingRange>> {
        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;

        self.workspace_for_path(&path)
            .folding_ranges(&path, encoding)
    }

    pub(crate) fn selection_ranges(
        &mut self,
        uri: &Url,
        positions: Vec<Position>,
    ) -> Option<Vec<SelectionRange>> {
        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;

        self.workspace_for_path(&path)
            .selection_ranges(&path, positions, encoding)
    }

    pub(crate) fn signature_help(
        &mut self,
        uri: &Url,
        position: Position,
    ) -> Option<SignatureHelp> {
        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;

        self.workspace_for_path(&path)
            .signature_help(&path, position, encoding)
    }

    pub(crate) fn prepare_rename(
        &mut self,
        uri: &Url,
        position: Position,
    ) -> Option<PrepareRenameResponse> {
        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;

        self.workspace_for_path(&path)
            .prepare_rename(&path, position, encoding)
    }

    pub(crate) fn rename(
        &mut self,
        uri: &Url,
        position: Position,
        new_name: &str,
    ) -> Option<WorkspaceEdit> {
        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;

        self.workspace_for_path(&path)
            .rename(&path, position, encoding, new_name)
    }

    pub(crate) fn document_links(&mut self, uri: &Url) -> Option<Vec<DocumentLink>> {
        let path = uri.to_file_path().ok()?;
        let encoding = self.encoding;

        self.workspace_for_path(&path)
            .document_links(&path, encoding)
    }
}

impl Workspace {
    fn document_highlights(
        &mut self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
    ) -> Option<Vec<DocumentHighlight>> {
        catch_unwind(AssertUnwindSafe(|| {
            let highlights = self.compilation.document_highlights_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
            );

            Some(lsp_document_highlights(
                &highlights,
                self.compilation.source_map(),
                encoding,
            ))
        }))
        .ok()
        .flatten()
    }

    fn folding_ranges(
        &mut self,
        path: &Path,
        encoding: PositionEncoding,
    ) -> Option<Vec<FoldingRange>> {
        catch_unwind(AssertUnwindSafe(|| {
            let ranges = self.compilation.folding_ranges(path);

            Some(lsp_folding_ranges(
                &ranges,
                self.compilation.source_map(),
                encoding,
            ))
        }))
        .ok()
        .flatten()
    }

    fn selection_ranges(
        &mut self,
        path: &Path,
        positions: Vec<Position>,
        encoding: PositionEncoding,
    ) -> Option<Vec<SelectionRange>> {
        catch_unwind(AssertUnwindSafe(|| {
            let positions = positions
                .into_iter()
                .map(|position| SourcePosition::new(position.line, position.character))
                .collect::<Vec<_>>();
            let ranges = self
                .compilation
                .selection_ranges(path, &positions, encoding);

            Some(lsp_selection_ranges(
                &ranges,
                self.compilation.source_map(),
                encoding,
            ))
        }))
        .ok()
        .flatten()
    }

    fn signature_help(
        &mut self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
    ) -> Option<SignatureHelp> {
        catch_unwind(AssertUnwindSafe(|| {
            let help = self.compilation.signature_help_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
            )?;

            Some(lsp_signature_help(&help))
        }))
        .ok()
        .flatten()
    }

    fn prepare_rename(
        &mut self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
    ) -> Option<PrepareRenameResponse> {
        catch_unwind(AssertUnwindSafe(|| {
            let span = self.compilation.prepare_rename_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
            )?;

            lsp_prepare_rename(span, self.compilation.source_map(), encoding)
        }))
        .ok()
        .flatten()
    }

    fn rename(
        &mut self,
        path: &Path,
        position: Position,
        encoding: PositionEncoding,
        new_name: &str,
    ) -> Option<WorkspaceEdit> {
        catch_unwind(AssertUnwindSafe(|| {
            let edits = self.compilation.rename_at_position(
                path,
                SourcePosition::new(position.line, position.character),
                encoding,
                new_name,
            );
            if edits.is_empty() {
                return None;
            }

            lsp_workspace_edit(&edits, self.compilation.source_map(), encoding)
        }))
        .ok()
        .flatten()
    }

    fn document_links(
        &mut self,
        path: &Path,
        encoding: PositionEncoding,
    ) -> Option<Vec<DocumentLink>> {
        catch_unwind(AssertUnwindSafe(|| {
            let links = self.compilation.document_links(path);

            Some(lsp_document_links(
                &links,
                self.compilation.source_map(),
                encoding,
            ))
        }))
        .ok()
        .flatten()
    }
}
