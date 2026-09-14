//! Portable evidence records. Collecting and verifying evidence is separate.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::diagnostics::{DiagnosticPath, DiagnosticSource};

use super::{ExecutionEngine, ExecutionMode, SourceLocation, SourceSpan, TargetReference};

/// An algorithm-qualified digest, without assumptions about a cache encoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fingerprint {
    /// Stable digest algorithm identifier, such as `sha256`.
    pub algorithm: String,
    /// Digest encoded according to that algorithm's portable convention.
    pub value: String,
}

/// Evidence for a fact, declaration, extraction, or execution.
///
/// Producers record exact tool versions and portable inputs, never timestamps,
/// process IDs, connection data, environment values, or checkout directories.
/// Missing observations stay absent rather than being inferred from the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// Activity responsible for the value.
    pub activity: ProvenanceActivity,
    /// Configuration-relative or repository-relative origin when known.
    pub source: Option<DiagnosticSource>,
    /// Source-file byte range when known.
    pub span: Option<SourceSpan>,
    /// Component identities mapped to exact versions, sorted by identity.
    pub tools: BTreeMap<String, String>,
}

/// Activity-specific provenance without transient runtime identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ProvenanceActivity {
    /// An explicit declaration or source observation.
    Declaration,
    /// API extraction from an explicit target.
    Extraction {
        /// Package-scoped source target.
        target: TargetReference,
        /// Extraction behavior used for this result.
        mode: ExtractionMode,
        /// Complete sorted capability identifiers used by the extractor.
        capabilities: BTreeSet<String>,
        /// Parser components keyed by identity, in lexical order.
        parsers: BTreeMap<String, ParserProvenance>,
        /// Contributing inputs keyed first by repository ID, then relative path.
        inputs: BTreeMap<String, BTreeMap<DiagnosticPath, ExtractionInput>>,
    },
    /// Successful execution or restoration of an executed page result.
    Execution {
        /// Declared execution mode.
        mode: ExecutionMode,
        /// Engine used to produce the result.
        engine: ExecutionEngine,
        /// Observed kernel information, without launch commands or credentials.
        kernel: KernelProvenance,
        /// Whether the result was freshly executed or restored.
        origin: ExecutionOrigin,
        /// Declared environment evidence in configured input order.
        declared_environment_inputs: Vec<InputFingerprint>,
    },
}

/// Extraction modes understood by this schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtractionMode {
    /// Parse declared sources without importing or running their code.
    Static,
}

/// One parser component and its output-affecting settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParserProvenance {
    /// Exact component version.
    pub version: String,
    /// Semantic parser role, such as `python-syntax` or `docstring`.
    pub role: String,
    /// Canonically spelled settings, sorted by setting name.
    pub settings: BTreeMap<String, String>,
}

/// Evidence for one contributing extraction input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractionInput {
    /// Input classification, such as `python-stub`, `rd`, or `package-metadata`.
    pub kind: String,
    /// Original input content fingerprint.
    pub fingerprint: Fingerprint,
    /// Parser component identities, sorted lexically.
    pub parsers: BTreeSet<String>,
}

/// Source evidence attached to a specific fact, such as one signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceEvidence {
    /// Proven repository source and optional byte range.
    pub source: SourceLocation,
    /// Contribution made by the source.
    pub role: SourceRole,
    /// Parser component identities, empty for direct observations.
    pub parsers: BTreeSet<String>,
}

/// Semantic role of source evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceRole {
    /// Package metadata.
    Metadata,
    /// An export declaration.
    Export,
    /// A method or API registration.
    Registration,
    /// The entity's definition.
    Definition,
    /// A signature declaration, possibly in a stub.
    Signature,
    /// Documentation prose or structure.
    Documentation,
    /// Evidence supporting a diagnostic.
    Diagnostic,
}

/// One explicitly declared environment input and its observed content digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputFingerprint {
    /// Repository-relative input file.
    pub source: SourceLocation,
    /// Content fingerprint of that input.
    pub fingerprint: Fingerprint,
}

/// Selected kernel identity and versions reported by the executing kernel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KernelProvenance {
    /// Configured kernelspec name, not a filesystem path.
    pub name: String,
    /// Kernel-reported language.
    pub language: Option<String>,
    /// Kernel-reported language version.
    pub language_version: Option<String>,
    /// Kernel implementation version when available.
    pub version: Option<String>,
}

/// How a successful executed result was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionOrigin {
    /// Fresh execution during this build.
    Executed,
    /// Restoration of a previously executed result.
    Cache,
}
