//! Portable execution evidence and its separate local asset handles.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::configuration::ExecutionMode;
use crate::diagnostics::{Diagnostic, DiagnosticPath};
use crate::documents::AuthoredFormat;
use crate::ir::{
    AssetReference, CellOutput, Fingerprint, Provenance, SourceLocation, SourceSegment, SourceSpan,
};

use super::{EffectiveCellOptions, ExecutionDeadlines, ExecutionDefaults};

/// Portable authored page evidence, available even when no kernel runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPage {
    /// Repository-relative authored page; its source span is absent.
    pub source: SourceLocation,
    /// Owning content collection identifier.
    pub collection: String,
    /// Repository-relative page parent; `None` denotes the repository root.
    pub working_directory: Option<DiagnosticPath>,
    /// Fingerprint of the entire original authored source, including prose.
    pub source_fingerprint: Fingerprint,
    /// Authored format; engine requests must use QMD.
    pub format: AuthoredFormat,
    /// Normalized collection authority; engine requests must use `execute`.
    pub mode: ExecutionMode,
    /// Whether top-level document metadata vetoes all execution.
    pub page_veto: bool,
    /// Exact authored parser version, supplied by the producer.
    pub parser_version: String,
    /// Active authored option policy, initially `qmd-mvp-v1`.
    pub qmd_policy: String,
}

/// A completed page transformation and the assets awaiting caller publication.
///
/// This local envelope deliberately does not implement serialization. A successful
/// session has closed its channels and reaped its kernel before returning these
/// handles. The caller owns retained staging files and must publish or discard
/// them. The record is evidence, not proof that untrusted assets are safe.
///
/// ```compile_fail
/// use diplodocus::execution::PageExecutionResult;
/// fn require_serializable<T: serde::Serialize>() {}
/// require_serializable::<PageExecutionResult>();
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct PageExecutionResult {
    /// Portable result without staging paths or transient protocol identities.
    pub record: PageExecutionRecord,
    /// One local file per portable asset; order matches the record's asset table.
    pub staged_assets: Vec<StagedExecutionAsset>,
}

/// A portable page result; not the canonical execution-cache artifact envelope.
///
/// Producers preserve cell and output order, sort assets by digest, and fix
/// diagnostic order before assigning diagnostic indices. Deserialization checks
/// field types only; future consumers must validate all cross-record invariants,
/// output safety, and referenced assets before reuse or rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageExecutionRecord {
    /// Authored source and policy evidence.
    pub page: ExecutionPage,
    /// Effective document defaults with declaration origins.
    pub defaults: ExecutionDefaults,
    /// Every authored cell, including skipped cells, in increasing ordinal order.
    pub cells: Vec<CellExecutionResult>,
    /// Portable diagnostics; output references index this finalized vector.
    pub diagnostics: Vec<Diagnostic>,
    /// Distinct accepted assets sorted by digest; each must be referenced.
    pub assets: Vec<ExecutionAsset>,
    /// Successful execution or cache evidence, absent when no cell was executed.
    ///
    /// An unexecuted record has only skipped cells and no outputs or assets.
    /// Failed attempts never become a page record. Cache restoration retains the
    /// original evidence and changes only its execution activity's origin.
    pub provenance: Option<PageExecutionProvenance>,
}

/// One authored cell's final outcome after all page-level display updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellExecutionResult {
    /// Zero-based ordinal among all authored cells.
    pub ordinal: usize,
    /// Normalized authored language, when provided.
    pub language: Option<String>,
    /// Full authored fence range.
    pub span: SourceSpan,
    /// Exact authored segments, preserving their original ranges.
    pub source_segments: Vec<SourceSegment>,
    /// Exact submitted-byte fingerprint; absent for skipped cells.
    pub submitted_source_fingerprint: Option<Fingerprint>,
    /// Effective options, including label and each winning declaration's origin.
    pub options: EffectiveCellOptions,
    /// Successful, allowed-error, or skipped outcome.
    pub outcome: CellOutcome,
    /// Surviving final output slots in creation order; empty for skipped cells.
    pub outputs: Vec<ExecutionOutput>,
}

/// Outcome of an authored cell in a completed page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CellOutcome {
    /// The submitted cell completed successfully.
    Ok,
    /// The cell raised a language exception permitted by its effective options.
    AllowedError,
    /// No source was submitted and no outputs were produced.
    Skipped {
        /// Reason selected during preparation or kernel language matching.
        reason: CellSkipReason,
    },
}

/// Why an authored cell was not submitted in an otherwise authorized page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CellSkipReason {
    /// Effective `eval` is false.
    EvalFalse,
    /// The verified kernel language differs; takes precedence over `eval: false`.
    LanguageMismatch,
}

/// A final output slot and portable attribution around the shared output IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionOutput {
    /// Authored ordinal of the cell that owns this slot.
    pub owning_cell: usize,
    /// Authored ordinal of the cell that originally produced this slot.
    pub producing_cell: usize,
    /// Latest updating cell's authored ordinal, if this slot was replaced.
    pub updating_cell: Option<usize>,
    /// Creation ordinal within the owning cell, retaining gaps after clearing.
    pub slot: usize,
    /// Shared typed output; accepted representations remain in preference order.
    pub output: CellOutput,
    /// MIME names offered by the current payload, sorted lexicographically.
    pub offered_mime_types: BTreeSet<String>,
    /// Selected accepted MIME type, or absent for errors and unsupported output.
    pub selected_mime_type: Option<String>,
    /// One evidence record per accepted representation, in the same order.
    pub representations: Vec<RepresentationEvidence>,
    /// Indices into the owning page's diagnostic vector, in diagnostic order.
    pub diagnostic_indices: Vec<usize>,
}

/// Accepted representation evidence without transport payloads or rendering trust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepresentationEvidence {
    /// Accepted content fingerprint, distinct from the submitted source digest.
    pub content_fingerprint: Fingerprint,
    /// Authored ordinal that produced the current representation's content.
    pub producing_cell: usize,
    /// Applied sanitizer or validator policy, absent for ordinary plain text.
    pub policy: Option<String>,
}

/// Portable metadata for one distinct accepted execution asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionAsset {
    /// Content-addressed reference beneath the owning page's asset namespace.
    pub reference: AssetReference,
    /// Validated MIME type of the accepted bytes.
    pub media_type: String,
    /// Size of the accepted bytes, independent of encoding or transport framing.
    pub byte_size: u64,
}

/// A local staged asset awaiting publication, deliberately not serializable.
///
/// ```compile_fail
/// use diplodocus::execution::StagedExecutionAsset;
/// fn require_serializable<T: serde::Serialize>() {}
/// require_serializable::<StagedExecutionAsset>();
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct StagedExecutionAsset {
    /// Portable reference matching one entry in the page's asset table.
    pub reference: AssetReference,
    /// Local staged regular file; never included in portable provenance.
    pub path: PathBuf,
}

/// Full successful execution evidence supplementing the shared provenance schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageExecutionProvenance {
    /// Shared execution activity, produced through `ExecutionObservation`.
    ///
    /// Contains engine, observed kernel versions, origin, sorted environment
    /// fingerprints, and authored source context. Tool versions include
    /// `diplodocus` and the engine identity, both at the producing crate version.
    pub execution: Provenance,
    /// Fingerprint of the producing executable's bytes.
    pub engine_build_fingerprint: Fingerprint,
    /// Exact component identities and versions, keyed by unique semantic role.
    pub components: BTreeMap<String, ExecutionComponent>,
    /// Output and execution policies used to produce the result.
    pub policies: ExecutionPolicies,
    /// Portable launch and protocol evidence beyond the shared kernel record.
    pub kernel: KernelExecutionProvenance,
    /// Observed execution platform, not inferred while decoding a record.
    pub platform: ExecutionPlatform,
    /// Actual configured deadlines, including any injected test limits.
    pub deadlines_ms: ExecutionDeadlines,
}

/// One execution component fulfilling a role such as transport or sanitizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionComponent {
    /// Component identity, such as a crate or built-in adapter name.
    pub name: String,
    /// Exact observed implementation version, never a version range.
    pub version: String,
}

/// Versioned policies; authored QMD policy is recorded on the page itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPolicies {
    /// Session and failure policy.
    pub execution: String,
    /// Output selection policy.
    pub mime: String,
    /// Kernel HTML validation policy.
    pub html: String,
    /// SVG validation policy.
    pub svg: String,
}

impl Default for ExecutionPolicies {
    fn default() -> Self {
        Self {
            execution: "execution-mvp-v1".into(),
            mime: "mime-mvp-v1".into(),
            html: "html-mvp-v1".into(),
            svg: "svg-mvp-v1".into(),
        }
    }
}

/// Kernel observations without launch arguments, environment values, or ports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KernelExecutionProvenance {
    /// Fingerprint of the selected spec's normalized behavioral fields.
    pub spec_fingerprint: Fingerprint,
    /// Fingerprint of normalized launch data, including executable identity.
    pub launch_fingerprint: Fingerprint,
    /// Searched location classes in search order; exactly one is selected.
    pub search: Vec<KernelSearchLocation>,
    /// Selected spec's declared interrupt behavior.
    pub interrupt_mode: KernelInterruptMode,
    /// Kernel-reported implementation name.
    pub implementation: String,
    /// Exact kernel-reported protocol version, including its minor version.
    pub protocol_version: String,
}

/// A searched location's portable classification, without its local path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KernelSearchLocation {
    /// Search root class.
    pub class: KernelSearchClass,
    /// Zero-based root ordinal within the class.
    pub ordinal: usize,
    /// Whether this root supplied the selected spec.
    pub selected: bool,
}

/// Linux kernelspec search classes in the documented discovery policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KernelSearchClass {
    /// An entry in `JUPYTER_PATH`.
    JupyterPath,
    /// The selected user data directory.
    UserData,
    /// The local system data directory.
    SystemLocal,
    /// The system data directory.
    System,
}

/// Interrupt delivery declared by the selected kernelspec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KernelInterruptMode {
    /// Signal the owned process group.
    Signal,
    /// Send a control-channel interrupt request.
    Message,
}

/// Producer-reported platform identity for reproducibility evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPlatform {
    /// Operating system identifier, initially `linux`.
    pub os: String,
    /// Architecture identifier.
    pub architecture: String,
    /// Full target triple of the producing build.
    pub target: String,
}
