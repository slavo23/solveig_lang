//! Conversion from driver completion items into LSP completion payloads.

use lsp_types::{CompletionItem, CompletionItemKind, Documentation, MarkupContent, MarkupKind};
use solveig_driver::{SourceCompletionItem, SourceCompletionItemKind};

pub(crate) fn lsp_completion_items(items: &[SourceCompletionItem]) -> Vec<CompletionItem> {
    items.iter().map(lsp_completion_item).collect()
}

fn lsp_completion_item(item: &SourceCompletionItem) -> CompletionItem {
    CompletionItem {
        label: item.label().to_string(),
        kind: Some(lsp_completion_item_kind(item.kind())),
        detail: item.detail().map(ToString::to_string),
        documentation: item.documentation().map(|value| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: value.to_string(),
            })
        }),
        ..CompletionItem::default()
    }
}

fn lsp_completion_item_kind(kind: SourceCompletionItemKind) -> CompletionItemKind {
    match kind {
        SourceCompletionItemKind::Variable => CompletionItemKind::VARIABLE,
        SourceCompletionItemKind::Function => CompletionItemKind::FUNCTION,
        SourceCompletionItemKind::Constructor => CompletionItemKind::CONSTRUCTOR,
        SourceCompletionItemKind::Type => CompletionItemKind::CLASS,
        SourceCompletionItemKind::Class => CompletionItemKind::INTERFACE,
        SourceCompletionItemKind::Method => CompletionItemKind::METHOD,
        SourceCompletionItemKind::Module => CompletionItemKind::MODULE,
        SourceCompletionItemKind::Keyword => CompletionItemKind::KEYWORD,
    }
}
