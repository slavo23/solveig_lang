//! Conversion from driver navigation payloads into LSP locations and symbols.

use async_lsp::lsp_types::{DocumentSymbol, Location, SymbolInformation, SymbolKind};
use solveig_base::{FileSpan, PositionEncoding, SourceMap};
use solveig_ide::{SourceSymbol, SourceSymbolKind};

use crate::diagnostics::{lsp_range, uri_for_file};

pub(crate) fn lsp_location(
    span: FileSpan,
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Option<Location> {
    Some(Location::new(
        uri_for_file(source_map, span.file)?,
        lsp_range(source_map, span, encoding),
    ))
}

pub(crate) fn lsp_document_symbols(
    symbols: &[SourceSymbol],
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Vec<DocumentSymbol> {
    symbols
        .iter()
        .filter_map(|symbol| lsp_document_symbol(symbol, source_map, encoding))
        .collect()
}

pub(crate) fn lsp_symbol_information(
    symbols: &[SourceSymbol],
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Vec<SymbolInformation> {
    symbols
        .iter()
        .filter_map(|symbol| lsp_symbol_info(symbol, source_map, encoding))
        .collect()
}

#[allow(deprecated)]
fn lsp_document_symbol(
    symbol: &SourceSymbol,
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Option<DocumentSymbol> {
    Some(DocumentSymbol {
        name: symbol.name().to_string(),
        detail: None,
        kind: symbol_kind(symbol.kind()),
        tags: None,
        deprecated: None,
        range: lsp_range(source_map, symbol.file_span(), encoding),
        selection_range: lsp_range(source_map, symbol.selection_span(), encoding),
        children: Some(lsp_document_symbols(
            symbol.children(),
            source_map,
            encoding,
        )),
    })
}

#[allow(deprecated)]
fn lsp_symbol_info(
    symbol: &SourceSymbol,
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Option<SymbolInformation> {
    let location = lsp_location(symbol.selection_span(), source_map, encoding)?;

    Some(SymbolInformation {
        name: symbol.name().to_string(),
        kind: symbol_kind(symbol.kind()),
        tags: None,
        deprecated: None,
        location,
        container_name: symbol.container_name().map(str::to_string),
    })
}

fn symbol_kind(kind: SourceSymbolKind) -> SymbolKind {
    match kind {
        SourceSymbolKind::Function => SymbolKind::FUNCTION,
        SourceSymbolKind::Type | SourceSymbolKind::TypeAlias => SymbolKind::STRUCT,
        SourceSymbolKind::Constructor => SymbolKind::CONSTRUCTOR,
        SourceSymbolKind::Class => SymbolKind::CLASS,
        SourceSymbolKind::Method => SymbolKind::METHOD,
    }
}
