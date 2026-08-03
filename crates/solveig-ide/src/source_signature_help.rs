//! IDE-owned signature help payloads for editor integrations.

/// Represents one callable signature shown by editor signature help.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSignatureInformation {
    label: String,
    parameters: Vec<String>,
    documentation: Option<String>,
}

impl SourceSignatureInformation {
    pub(super) fn new(label: impl Into<String>, parameters: Vec<String>) -> Self {
        Self {
            label: label.into(),
            parameters,
            documentation: None,
        }
    }

    pub(super) fn with_optional_documentation(mut self, documentation: Option<String>) -> Self {
        self.documentation = documentation;
        self
    }

    /// Returns the signature label displayed by editor clients.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns display labels for individual parameters.
    pub fn parameters(&self) -> &[String] {
        &self.parameters
    }

    /// Returns documentation attached to the callable, when available.
    pub fn documentation(&self) -> Option<&str> {
        self.documentation.as_deref()
    }
}

/// Represents source-level signature help at a cursor position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSignatureHelp {
    signatures: Vec<SourceSignatureInformation>,
    active_signature: u32,
    active_parameter: u32,
}

impl SourceSignatureHelp {
    pub(super) fn new(
        signature: SourceSignatureInformation,
        active_signature: u32,
        active_parameter: u32,
    ) -> Self {
        Self {
            signatures: vec![signature],
            active_signature,
            active_parameter,
        }
    }

    /// Returns the available signatures.
    pub fn signatures(&self) -> &[SourceSignatureInformation] {
        &self.signatures
    }

    /// Returns the selected signature index.
    pub const fn active_signature(&self) -> u32 {
        self.active_signature
    }

    /// Returns the selected parameter index.
    pub const fn active_parameter(&self) -> u32 {
        self.active_parameter
    }
}
