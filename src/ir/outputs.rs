//! Portable cell output records, separate from rendering trust.

use serde::{Deserialize, Serialize};

use crate::diagnostics::DiagnosticPath;

use super::{Block, Fingerprint, Provenance};

/// One output event in a cell's event order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellOutput {
    /// Stream, display, or error information.
    pub kind: CellOutputKind,
    /// Supported representations in producer-selected preference order.
    pub representations: Vec<OutputRepresentation>,
    /// Producing execution evidence in source order.
    pub provenance: Vec<Provenance>,
}

/// The semantic kind of an output event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CellOutputKind {
    /// Preformatted stdout or stderr; ordinary stream text is never Markdown.
    Stream {
        /// Producing stream.
        stream: StreamName,
    },
    /// Rich display data or an expression result.
    Display,
    /// An execution error with portable, control-sequence-free traceback text.
    Error {
        /// Stable language exception name.
        name: String,
        /// Exception message; producers normalize known runtime paths.
        message: String,
        /// Frames in reported order; producers remove machine-specific context.
        traceback: Vec<String>,
    },
}

/// Standard output streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StreamName {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// A typed portable representation; decoding does not grant rendering trust.
///
/// Output adapters must enforce MIME policies, inert generated Markdown, and
/// asset validation before rendering. This schema does not perform those steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OutputRepresentation {
    /// Text that a renderer must escape and display without Markdown parsing.
    PlainText {
        /// Original supported media type, normally `text/plain`.
        media_type: String,
        /// Literal preformatted text.
        text: String,
    },
    /// Parsed Markdown fragment with fragment-relative source spans.
    MarkdownBlocks {
        /// Original media type, normally `text/markdown`.
        media_type: String,
        /// Structured fragment blocks in source order.
        blocks: Vec<Block>,
    },
    /// A content-addressed local binary or validated vector asset.
    Asset {
        /// Supported asset media type.
        media_type: String,
        /// Portable asset reference, without an absolute output directory.
        asset: AssetReference,
    },
    /// A producer's serialized claim of sanitized HTML, still untrusted on read.
    ///
    /// The wire tag records the producer's representation kind. The Rust value
    /// remains an explicitly unvalidated candidate, so a cache deserializer
    /// cannot manufacture trusted HTML. A future in-process sanitizer must
    /// revalidate it against the active policy before producing a rendering value.
    #[serde(rename = "sanitized-html")]
    HtmlCandidate {
        /// Original media type, normally `text/html`.
        media_type: String,
        /// Unvalidated serialized markup.
        html: UnvalidatedHtml,
        /// Policy identifier used by the producer.
        policy: String,
        /// Sanitizer component identity and exact version used by the producer.
        sanitizer: SanitizerProvenance,
    },
}

/// Serialized markup without rendering trust.
///
/// This type deliberately has no conversion to trusted HTML or generic display
/// implementation. Deserialization retains the bytes for later sanitization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnvalidatedHtml(String);

impl UnvalidatedHtml {
    /// Retain serialized markup as explicitly untrusted input.
    pub fn new(markup: impl Into<String>) -> Self {
        Self(markup.into())
    }

    /// Read untrusted markup for validation by the active sanitizer.
    pub fn as_untrusted_str(&self) -> &str {
        &self.0
    }
}

/// The producer's sanitizer identity; this record alone conveys no trust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SanitizerProvenance {
    /// Stable sanitizer component name.
    pub name: String,
    /// Exact sanitizer implementation version.
    pub version: String,
}

/// A local asset named independently of the build's output directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetReference {
    /// Normalized path relative to the snapshot root.
    pub path: DiagnosticPath,
    /// Accepted asset content fingerprint, used to derive its local name.
    pub fingerprint: Fingerprint,
}
