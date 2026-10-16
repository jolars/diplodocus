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
    /// Native Rust doctests checked against the selected checkout and features.
    RustDoctests {
        /// Owning Rust extraction target.
        target: TargetReference,
        /// Versioned native execution and comparison contract.
        policy: String,
        /// Cargo package identity.
        package_name: String,
        /// Version read from Cargo metadata.
        package_version: String,
        /// Observed compiler version and commit.
        compiler_version: String,
        /// Resolved dependency versions without local paths.
        dependencies: BTreeMap<String, String>,
        /// Effective Cargo features in sorted order.
        features: Vec<String>,
        /// Host target triple used for compilation and execution.
        host: String,
        /// Maintained source and environment input fingerprints.
        inputs: Vec<InputFingerprint>,
        /// Native example outcomes in discovery order.
        examples: Vec<RustDoctestOutcome>,
    },
    /// Completed Python or Julia API doctests in the supplied checkout.
    ApiExamples {
        /// Owning workspace package.
        package: String,
        /// Native language adapter.
        language: String,
        /// Versioned preparation and comparison contract.
        policy: String,
        /// Observed interpreter version.
        runtime_version: String,
        /// Verified checkout identity.
        package_name: String,
        /// Version read from the loaded checkout's metadata.
        package_version: String,
        /// Loaded dependency versions without library paths.
        dependencies: BTreeMap<String, String>,
        /// Decoded documentation source and fingerprint.
        input: InputFingerprint,
        /// Example outcomes in source order.
        examples: Vec<ApiExampleOutcome>,
    },
    /// Julia type reflection in the verified supplied checkout.
    JuliaRuntime {
        /// Owning extraction target.
        target: TargetReference,
        /// Native helper contract.
        policy: String,
        /// Observed Julia interpreter version.
        julia_version: String,
        /// Loaded package identity and version.
        package_name: String,
        /// Loaded package UUID.
        package_uuid: String,
        /// Loaded package version.
        package_version: String,
        /// Loaded dependency UUIDs and versions, without library paths.
        dependencies: BTreeMap<String, String>,
    },
    /// Completed native R Markdown execution in the prepared checkout.
    RMarkdown {
        /// Owning workspace package.
        package: String,
        /// Native helper contract.
        policy: String,
        /// Observed interpreter version.
        r_version: String,
        /// Observed knitr version.
        knitr_version: String,
        /// Observed checkout version.
        package_version: String,
        /// Loaded namespace versions without library paths.
        dependencies: BTreeMap<String, String>,
        /// Original article source and fingerprint.
        input: InputFingerprint,
        /// Evaluated Markdown fingerprint.
        output: Fingerprint,
        /// Chunk outcomes in source order, including explicit omissions.
        chunks: Vec<RMarkdownChunkOutcome>,
        /// Inline evaluations in knitr processing order.
        inline: Vec<RMarkdownInlineOutcome>,
    },
    /// An explicit declaration or source observation.
    Declaration,
    /// A decoded documentation range mapped back to its source literal.
    DecodedDocumentation {
        /// Zero-based UTF-8 byte range in the decoded document.
        decoded_span: SourceSpan,
    },
    /// Inert parsing of Markdown output from an authored cell.
    ///
    /// The enclosing provenance source and span identify the producing page and
    /// authored cell fence. Fragment block and diagnostic ranges are relative to
    /// the output identified here, not that page. No execution is implied.
    GeneratedMarkdown {
        /// Owning authored content collection ID.
        collection: String,
        /// Zero-based producing cell ordinal in authored source order.
        cell: usize,
        /// Zero-based output-slot ordinal within the producing cell.
        output: usize,
    },
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
    /// Native R metadata and citation completion from the supplied checkout.
    PackageMetadata {
        /// Workspace package identity.
        package: String,
        /// Observed R interpreter version.
        r_version: String,
        /// Native helper policy used to complete the results.
        policy: String,
        /// Exact source bytes supplied to native helpers.
        inputs: Vec<InputFingerprint>,
    },
    /// Completed native R help-topic examples from the supplied checkout.
    RdExamples {
        /// Workspace package identity.
        package: String,
        /// Native example adapter policy.
        policy: String,
        /// Observed interpreter version.
        r_version: String,
        /// Observed checkout package version.
        package_version: String,
        /// Loaded namespace versions, excluding local library paths.
        dependencies: BTreeMap<String, String>,
        /// Original help-topic input.
        input: InputFingerprint,
        /// Executed expressions in source order.
        executed: Vec<String>,
        /// Explicit illustrative regions excluded by native Rd conversion.
        illustrative: Vec<String>,
        /// Conditional omissions and their evaluated reasons.
        skipped: Vec<String>,
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
        /// Declared environment evidence sorted by repository and relative path.
        declared_environment_inputs: Vec<InputFingerprint>,
    },
}

/// One checked transcript expression or script in an API document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiExampleOutcome {
    /// Example range in the decoded documentation.
    pub span: SourceSpan,
    /// Submitted source, including supported comparison directives.
    pub source: String,
    /// Authored expected output.
    pub expected: String,
    /// Actual literal output, empty for explicitly skipped examples.
    pub actual: String,
    /// Executed, expected-error, or explicit-skip.
    pub outcome: String,
    /// Explanation of an explicit omission.
    pub reason: Option<String>,
    /// Effective language-specific comparison and setup options.
    pub options: BTreeMap<String, String>,
}

/// Native chunk evidence independent of a Jupyter kernel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RMarkdownChunkOutcome {
    /// Full chunk range in the original R Markdown input.
    pub span: SourceSpan,
    /// Native chunk label.
    pub label: String,
    /// Effective supported options observed before execution.
    pub options: BTreeMap<String, String>,
    /// Executed, illustrative, conditional-skip, or allowed-error.
    pub outcome: String,
    /// Explanation of an explicit omission or permitted error.
    pub reason: Option<String>,
}

/// One inline R result attributed to the original expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RMarkdownInlineOutcome {
    /// Expression range in the original R Markdown source.
    pub span: SourceSpan,
    /// Native Markdown representation of the evaluated value.
    pub value: String,
}

/// Extraction modes understood by this schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtractionMode {
    /// Parse declared sources without importing or running their code.
    Static,
    /// Resolve declarations with native compiler tooling, allowing local builds.
    Compiler,
}

/// One Rust example discovered and checked by the native doctest runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustDoctestOutcome {
    /// Native test name, retained to reconcile discovery with execution.
    pub name: String,
    /// Original maintained file when native attribution is available.
    pub source: Option<SourceLocation>,
    /// One-based source line reported by rustdoc.
    pub line: usize,
    /// Original doctest code, including native setup syntax.
    pub code: String,
    /// Effective native doctest attributes.
    pub attributes: BTreeMap<String, String>,
    /// Checked native outcome.
    pub outcome: RustDoctestStatus,
    /// Output reported by the native runner; successful stdout may be absent.
    pub output: String,
    /// Native reason for an explicit omission.
    pub reason: Option<String>,
}

/// Successful Rust doctest outcomes preserve compilation and execution semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RustDoctestStatus {
    /// Compiled and executed successfully.
    Executed,
    /// Compiled successfully under `no_run`.
    CompileOnly,
    /// Matched an explicit compilation failure or panic expectation.
    ExpectedError,
    /// Omitted by an explicit native ignore attribute.
    ExplicitSkip,
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
