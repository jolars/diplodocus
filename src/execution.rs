//! Page-level authored execution contracts and an internal Jupyter runner.
//!
//! The internal Linux adapter executes prepared cells in one supervised session
//! per page. An internal incremental reducer converts kernel events into typed
//! outputs through a representation validator, including inert Markdown MIME
//! and as-is stdout fragments. A page-scoped asset store validates and stages
//! PNG, JPEG, and inert SVG figures. A supervised cell-consumption hook prevents
//! further execution after a fatal output failure. Active HTML and fragment image
//! validation feeds immutable shared record carriers. The production
//! [`ExecutionEngine`] implementation remains future work.
//! [`crate::documents::prepare_collection_document`]
//! validates collection authority and prepares QMD cells without I/O. Callers
//! must also authorize the current command before dispatching execution.
//! Parsing, runtime option enforcement, rendering, and cache publication are
//! separate stages. This library interface is not a plugin registration API.
//!
//! Portable records describe producer evidence, not verified rendering trust or
//! the final cache wire format. Constructing or decoding them performs no I/O
//! and does not validate execution eligibility, assets, or output safety.

use std::collections::BTreeSet;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::ir::{CodeCell, InputFingerprint};

pub mod assets;
mod failure;
mod figures;
pub mod identity;
#[cfg(target_os = "linux")]
#[allow(
    dead_code,
    reason = "The public engine awaits validated output conversion."
)]
mod jupyter;
mod options;
pub mod output_safety;
mod records;
pub mod validated;

pub use failure::*;
pub use figures::*;
pub use options::*;
pub use records::*;
pub use validated::{
    PageExecutionResult, PreparedExecution, ValidatedPage, ValidatedRepresentationRef,
};

/// Completion of a page session, including its bounded cleanup.
pub type ExecutionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<PageExecutionResult, ExecutionFailure>> + Send + 'a>>;

/// A cooperative cancellation signal; completion requests interruption and cleanup.
pub type ExecutionCancellation<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// One engine's page-level execution boundary.
///
/// Implementations own one session per call, submit cells sequentially in authored
/// order, and preserve state between submitted cells. They convert transport
/// values immediately into Diplodocus-owned types. Success requires completed
/// kernel cleanup and validated output; no owned kernel may remain live after a
/// successful return. Failures attempt to discard uncommitted assets, report any
/// incomplete cleanup, and never return a publishable page.
///
/// On cancellation, callers keep polling the returned future through interruption
/// and cleanup. Dropping it is not a supported cancellation mechanism: a future's
/// destructor cannot perform asynchronous cleanup. Session supervision must also
/// handle unexpected drops when a production engine is implemented.
pub trait ExecutionEngine: Send + Sync {
    /// Describe implemented features without I/O, discovery, or runtime probes.
    fn capabilities(&self) -> ExecutionCapabilities;

    /// Describe external requirements without checking whether they are installed.
    fn requirements(&self) -> ExecutionRequirements;

    /// Execute one prepared, explicitly authorized page.
    ///
    /// The caller has checked QMD collection authority, validated every option,
    /// applied the page veto, and found at least one potentially executable cell.
    /// `check`, `mode = "never"`, vetoed pages, and pages with no candidates must
    /// never reach this method. The engine discovers only the requested kernel,
    /// then selects cells by its verified language. A language mismatch can leave
    /// every cell skipped without a launch or execution provenance.
    ///
    /// Both success and failure run bounded session cleanup before returning;
    /// incomplete cleanup is itself a failure.
    fn execute_page<'a>(
        &'a self,
        context: ExecutionContext<'a>,
        request: &'a PageExecutionRequest,
    ) -> ExecutionFuture<'a>;
}

/// Local execution inputs, deliberately excluded from portable serialization.
///
/// Paths have already been resolved by the caller. The repository is the input
/// boundary, the page's parent is the working directory, and the staging
/// directory is a separate page-scoped output boundary. Engines must recheck
/// filesystem boundaries when reading or writing assets.
///
/// ```compile_fail
/// use diplodocus::execution::ExecutionContext;
/// fn require_serializable<T: serde::Serialize>() {}
/// require_serializable::<ExecutionContext<'static>>();
/// ```
pub struct ExecutionContext<'a> {
    /// Absolute canonical root of the collection's repository.
    pub repository_root: PathBuf,
    /// Absolute canonical path of the authored page within that repository.
    pub page_path: PathBuf,
    /// Local directory for uncommitted execution assets.
    pub asset_staging_directory: PathBuf,
    /// Active phase limits, including shorter limits injected by tests.
    pub deadlines: ExecutionDeadlines,
    /// Await alongside startup, execution, and other active session operations.
    pub cancellation: ExecutionCancellation<'a>,
}

impl fmt::Debug for ExecutionContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutionContext")
            .field("repository_root", &self.repository_root)
            .field("page_path", &self.page_path)
            .field("asset_staging_directory", &self.asset_staging_directory)
            .field("deadlines", &self.deadlines)
            .finish_non_exhaustive()
    }
}

/// Declarative engine capabilities; these sets make no claim about installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionCapabilities {
    /// Normalized language names in lexical order.
    pub languages: BTreeSet<String>,
    /// Supported output MIME names in lexical order, not preference order.
    pub media_types: BTreeSet<String>,
    /// Features actually implemented by this engine.
    pub features: BTreeSet<ExecutionFeature>,
}

/// Execution features an engine can advertise independently of its transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionFeature {
    /// Cells can retain state in one page-scoped session.
    PageSession,
    /// Stdout and stderr are retained in event order.
    Streams,
    /// Display and result MIME bundles are collected.
    RichOutput,
    /// Updates can replace surviving output slots, including in earlier cells.
    DisplayUpdates,
    /// Immediate and deferred output clearing are supported.
    ClearOutput,
    /// Allowed language errors remain outputs while later cells run.
    AllowedErrors,
    /// Failed or canceled execution can be interrupted before shutdown.
    Interruption,
}

/// External prerequisites for an engine; reading this record never probes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRequirements {
    /// Supported operating system identifiers, such as `linux`.
    pub operating_systems: BTreeSet<String>,
    /// Required protocol of the explicitly selected, caller-installed kernel.
    pub protocol: KernelProtocolRequirement,
}

/// Kernel protocol compatibility required before any source is submitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KernelProtocolRequirement {
    /// Protocol identity, such as `jupyter`.
    pub name: String,
    /// Required major version; exact observed versions belong to provenance.
    pub major: u32,
}

/// A caller-prepared page; constructing this record does not authorize execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageExecutionRequest {
    /// Authored page identity and source evidence.
    pub page: ExecutionPage,
    /// Explicit configured selector, never a path or an inferred language name.
    pub kernel: String,
    /// Effective document defaults with winning declaration origins.
    pub defaults: ExecutionDefaults,
    /// Every authored cell in source order, including nested and skipped cells.
    pub cells: Vec<PreparedCell>,
    /// Declared environment evidence sorted by repository and relative path.
    pub declared_environment_inputs: Vec<InputFingerprint>,
}

/// Original cell input paired with caller-normalized options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedCell {
    /// Zero-based ordinal among all authored cells, including nested cells.
    pub ordinal: usize,
    /// Original parsed cell, retaining all declarations and source segments.
    ///
    /// Outputs must be empty. Its source is submitted without further whitespace
    /// normalization. Language selection follows verified kernel discovery.
    pub cell: CodeCell,
    /// Validated effective values and their origins; these do not grant authority.
    pub options: EffectiveCellOptions,
}
