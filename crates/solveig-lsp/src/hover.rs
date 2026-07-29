//! Conversion from driver semantic hover payloads into LSP hover responses.

use lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};
use solveig_base::{PositionEncoding, SourceMap};
use solveig_driver::HoverInfo;

use crate::diagnostics::lsp_range;

pub(crate) fn lsp_hover(
    info: &HoverInfo,
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Hover {
    let mut value = format!("```solveig\n{}\n```", info.signature());

    if let Some(documentation) = info.documentation()
        && !documentation.trim().is_empty()
    {
        value.push_str("\n\n");
        value.push_str(documentation);
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(lsp_range(source_map, info.file_span(), encoding)),
    }
}
