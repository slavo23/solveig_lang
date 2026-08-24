//! LSP initialization and request-option conversion into server configuration.

use async_lsp::lsp_types::{FormattingOptions, FormattingProperty, InitializeParams};

use crate::workspace::ServerConfig;

pub(crate) fn server_config_from_initialize(params: &InitializeParams) -> ServerConfig {
    let mut config = ServerConfig::default();

    if let Some(value) = params
        .initialization_options
        .as_ref()
        .and_then(|options| options.get("inlayHints"))
        .and_then(|options| options.get("enabled"))
        .and_then(|enabled| enabled.as_bool())
    {
        config.inlay_hints_enabled = value;
    }

    if let Some(value) = params
        .initialization_options
        .as_ref()
        .and_then(|options| options.get("diagnostics"))
        .and_then(|options| options.get("inferredSignatures"))
        .and_then(|options| options.get("enabled"))
        .and_then(|enabled| enabled.as_bool())
    {
        config.inferred_signature_hints_enabled = value;
    }

    if let Some(value) = params
        .initialization_options
        .as_ref()
        .and_then(|options| options.get("formatting"))
        .and_then(|options| options.get("lineWidth"))
        .and_then(|width| width.as_u64())
        .and_then(|width| usize::try_from(width).ok())
        .filter(|width| *width > 0)
    {
        config.format_line_width = value;
    }

    config
}

pub(crate) fn formatting_width(options: &FormattingOptions, default: usize) -> usize {
    match options.properties.get("lineWidth") {
        Some(FormattingProperty::Number(width)) if *width > 0 => {
            usize::try_from(*width).unwrap_or(default)
        }
        _ => default,
    }
}
