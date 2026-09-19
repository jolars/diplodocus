//! Failure categories and portable phase limits, without runtime error strings.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEntity, DiagnosticSource, Severity,
};
use crate::ir::SourceLocation;

/// Normalized phase limits in milliseconds; these are limits, not observations.
///
/// Producers use monotonic deadlines and require positive values. Construction
/// and deserialization do not enforce policy or start timers. Tests may inject
/// shorter limits, which must also appear in the resulting provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionDeadlines {
    /// Spawn through channel readiness and validated kernel information.
    pub startup: u64,
    /// Submission through both terminal messages; activity never extends it.
    pub cell: u64,
    /// First terminal message through its counterpart, capped by the cell limit.
    pub terminal_sync: u64,
    /// Interrupt delivery through idle or process exit.
    pub interrupt: u64,
    /// Graceful shutdown request, reply, and process exit.
    pub shutdown: u64,
    /// Termination of the process group before escalation to kill.
    pub termination: u64,
    /// Exit and reaping after kill.
    pub forced_exit: u64,
}

impl Default for ExecutionDeadlines {
    fn default() -> Self {
        Self {
            startup: 30_000,
            cell: 60_000,
            terminal_sync: 5_000,
            interrupt: 5_000,
            shutdown: 5_000,
            termination: 5_000,
            forced_exit: 5_000,
        }
    }
}

/// Phase identified by a timeout; elapsed time never enters portable diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionPhase {
    /// Kernel startup and readiness.
    Startup,
    /// Cell submission and completion.
    Cell,
    /// Synchronization of the two terminal messages.
    TerminalSync,
    /// Interruption of failed or canceled execution.
    Interrupt,
    /// Graceful shutdown.
    Shutdown,
    /// Process-group termination.
    Termination,
    /// Exit and reaping after forced kill.
    ForcedExit,
}

impl fmt::Display for ExecutionPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Startup => "startup",
            Self::Cell => "cell execution",
            Self::TerminalSync => "terminal synchronization",
            Self::Interrupt => "interruption",
            Self::Shutdown => "shutdown",
            Self::Termination => "process termination",
            Self::ForcedExit => "forced exit",
        })
    }
}

/// A failed attempt with no successful page record or publishable asset handles.
///
/// Keep the primary failure even when cleanup also fails. Diagnostic messages
/// must be portable; local transport and operating system errors must be
/// normalized before being included. This type does not serialize runtime state.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{kind}")]
pub struct ExecutionFailure {
    /// Failure that stopped the page attempt.
    pub kind: ExecutionFailureKind,
    /// Primary failure and preceding diagnostics in deterministic order.
    pub diagnostics: Vec<Diagnostic>,
    /// Additional cleanup failures; any entry prevents publication.
    pub cleanup_diagnostics: Vec<Diagnostic>,
}

/// Transport-independent failure categories, including the failing timeout phase.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ExecutionFailureKind {
    /// Kernel selection, launch, or readiness failed.
    #[error("The configured kernel could not be selected or started.")]
    Startup,
    /// Authentication, transport, protocol validation, or kernel liveness failed.
    #[error("Kernel communication or protocol validation failed.")]
    Protocol,
    /// The kernel requested unsupported interactive input.
    #[error("The kernel requested interactive input.")]
    InputRequested,
    /// A language exception was disallowed or the kernel aborted a cell.
    #[error("A cell failed without permission to continue.")]
    CellError,
    /// A monotonic phase deadline expired.
    #[error("Execution timed out during {phase}.")]
    Timeout {
        /// Phase whose deadline expired.
        phase: ExecutionPhase,
    },
    /// The caller requested cancellation.
    #[error("Page execution was canceled.")]
    Cancelled,
    /// An unrecoverable output conversion or validation failure occurred.
    ///
    /// Unsupported representations normally produce warnings and placeholders;
    /// their absence of a safe MIME alternative alone is not a page failure.
    #[error("Cell output could not be validated.")]
    OutputValidation,
    /// A generated asset escaped its declared input or output boundary.
    #[error("A generated asset is outside its declared boundary.")]
    AssetOutsideBoundary,
    /// A generated asset could not be found.
    #[error("A generated asset is missing.")]
    AssetMissing,
    /// The same asset digest identified inconsistent bytes or metadata.
    #[error("Execution assets conflict at the same content digest.")]
    AssetCollision,
    /// Session or staging cleanup failed.
    #[error("Execution cleanup could not be completed.")]
    Cleanup,
}

impl ExecutionFailureKind {
    /// Stable shared diagnostic code for this failure category.
    pub const fn diagnostic_code(self) -> DiagnosticCode {
        match self {
            Self::Startup => DiagnosticCode::ExecutionStartupFailed,
            Self::Protocol => DiagnosticCode::ExecutionProtocolFailed,
            Self::InputRequested => DiagnosticCode::ExecutionInputRequested,
            Self::CellError => DiagnosticCode::ExecutionCellFailed,
            Self::Timeout { .. } => DiagnosticCode::ExecutionTimeout,
            Self::Cancelled => DiagnosticCode::ExecutionCancelled,
            Self::OutputValidation => DiagnosticCode::ExecutionOutputFailed,
            Self::AssetOutsideBoundary => DiagnosticCode::GeneratedAssetOutsideBoundary,
            Self::AssetMissing => DiagnosticCode::GeneratedAssetMissing,
            Self::AssetCollision => DiagnosticCode::ExecutionAssetCollision,
            Self::Cleanup => DiagnosticCode::ExecutionCleanupFailed,
        }
    }

    /// Produce a portable error without copying raw runtime error messages.
    ///
    /// The supplied location identifies the page or producing cell. The content
    /// entity supplies ownership without guessing a collection-relative path
    /// from a repository-relative source path. Callers may attach related ranges.
    pub fn to_diagnostic(
        self,
        collection: impl Into<String>,
        source: SourceLocation,
    ) -> Diagnostic {
        let mut diagnostic =
            Diagnostic::new(self.diagnostic_code(), Severity::Error, self.to_string())
                .with_entity(DiagnosticEntity::Content {
                    id: collection.into(),
                })
                .with_source(DiagnosticSource::Repository {
                    repository: source.repository,
                    path: source.path,
                });
        diagnostic.span = source.span;
        diagnostic
    }
}
