//! Diagnostics produced while processing documentation.

use serde::{Deserialize, Serialize};

use crate::ir::SourceSpan;

/// Stable diagnostic identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticCode {
    /// Authored syntax is outside Diplodocus's supported profile.
    UnsupportedAuthoredSyntax,
    /// Panache reported malformed embedded YAML.
    InvalidEmbeddedYaml,
    /// Cell-option precedence has more than one winner.
    AmbiguousCellOption,
    /// Document metadata requests execution outside collection authority.
    DocumentExecutionNotAuthorized,
    /// A QMD document declares an unsupported metadata setting.
    UnsupportedQmdMetadata,
    /// A QMD metadata declaration has an invalid value type.
    InvalidQmdMetadata,
}

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// The document is retained, but the condition is visible.
    Warning,
    /// The source is invalid for the selected authored profile.
    Error,
}

/// A deterministic diagnostic tied to an authored-document span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Stable identifier.
    pub code: DiagnosticCode,
    /// Severity.
    pub severity: Severity,
    /// Human-readable explanation.
    pub message: String,
    /// Source range when available.
    pub span: Option<SourceSpan>,
    /// Additional declarations contributing to this diagnostic, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_spans: Vec<SourceSpan>,
}
