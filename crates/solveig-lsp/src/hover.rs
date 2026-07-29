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
    let signature = trim_hover_text(info.signature());
    let mut value = format!("```solveig\n{signature}\n```");

    if let Some(documentation) = info
        .documentation()
        .map(trim_hover_text)
        .filter(|documentation| !documentation.is_empty())
    {
        value.push_str("\n\n");
        value.push_str(&documentation);
    }

    if let Some(provenance) = info
        .provenance()
        .map(trim_hover_text)
        .filter(|provenance| !provenance.is_empty())
    {
        value.push_str("\n\n");
        value.push_str(&provenance);
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(lsp_range(source_map, info.file_span(), encoding)),
    }
}

fn trim_hover_text(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}
