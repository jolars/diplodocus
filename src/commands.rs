//! Testable operations invoked by the command-line interface.

use std::net::IpAddr;
use std::path::PathBuf;

use thiserror::Error;

use crate::assembly::{AssemblyError, assemble_workspace};
use crate::diagnostics::{Diagnostic, DiagnosticSource, Severity};
use crate::validation::{ResolutionError, resolve_workspace};

mod pipeline;
mod preview;
pub use pipeline::extract_with;
pub use preview::serve_with;

/// Options for checking a documentation workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOptions {
    /// Path to the workspace configuration file.
    pub config: PathBuf,
}

/// Options for building a static documentation site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOptions {
    /// Path to the workspace configuration file.
    pub config: PathBuf,
    /// Directory in which to write the generated site.
    pub output: PathBuf,
}

/// Options for building and serving a documentation site locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeOptions {
    /// Path to the workspace configuration file.
    pub config: PathBuf,
    /// Directory in which to write the generated site.
    pub output: PathBuf,
    /// Address on which to serve the generated site.
    pub host: IpAddr,
    /// Port on which to serve the generated site.
    pub port: u16,
}

/// An error returned by a Diplodocus command operation.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CommandError {
    /// Static source assembly failed.
    #[error(transparent)]
    Assembly(#[from] AssemblyError),
    /// Semantic or local reference validation failed.
    #[error(transparent)]
    Resolution(#[from] ResolutionError),
    /// Portable snapshot publication or loading failed.
    #[error(transparent)]
    Snapshot(#[from] crate::snapshots::SnapshotError),
    /// Site construction, rendering, or publication failed.
    #[error(transparent)]
    Site(#[from] crate::site::SiteError),
    /// Command I/O failed.
    #[error("command I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// An output would replace declared source inputs or the input snapshot.
    #[error("output overlaps a declared input")]
    InputOverlap,
    /// No execution backend is available on this platform.
    #[error("authored execution is unsupported on this platform")]
    UnsupportedExecution,
}

impl CommandError {
    /// Cleanup failures that accompany the primary execution failure.
    pub fn cleanup_diagnostics(&self) -> &[Diagnostic] {
        fn cleanup(error: &AssemblyError) -> &[Diagnostic] {
            match error {
                AssemblyError::Execution(failure) => &failure.cleanup_diagnostics,
                AssemblyError::Cleanup { cause, .. } => cleanup(cause),
                _ => &[],
            }
        }
        match self {
            Self::Assembly(error) => cleanup(error),
            _ => &[],
        }
    }
    /// Portable source diagnostics when the operation reached source validation.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::Assembly(error) => assembly_diagnostics(error),
            Self::Resolution(error) => error.diagnostics(),
            _ => &[],
        }
    }
}

/// Successful static check, including nonfatal diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// Deterministically ordered source and validation warnings.
    pub diagnostics: Vec<Diagnostic>,
}

/// Build a static documentation site.
pub fn build(options: BuildOptions) -> Result<(), CommandError> {
    pipeline::runtime()?.block_on(pipeline::build_attempt(
        &options,
        crate::execution::ExecutionDeadlines::default(),
        Box::pin(preview::termination()),
    ))?;
    Ok(())
}

/// Check a documentation workspace without writing a site.
pub fn check(options: CheckOptions) -> Result<CheckReport, CommandError> {
    let sources = assemble_workspace(&options.config)?;
    let resolved = resolve_workspace(&sources)?;
    Ok(CheckReport {
        diagnostics: resolved.diagnostics().iter().cloned().collect(),
    })
}

/// Format a source diagnostic on one terminal-safe line.
pub fn format_diagnostic(diagnostic: &Diagnostic) -> String {
    let severity = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    let location = match &diagnostic.source {
        Some(DiagnosticSource::Configuration { path }) => path.as_str().to_owned(),
        Some(DiagnosticSource::Repository { repository, path }) => {
            format!("{repository}:{}", path.as_str())
        }
        None => String::new(),
    };
    let span = diagnostic
        .span
        .map(|s| format!(" bytes {}..{}", s.start, s.end))
        .unwrap_or_default();
    let line = format!(
        "{severity}[{}] {location}{span}: {}",
        diagnostic.code.as_str(),
        diagnostic.message
    );
    line.chars()
        .flat_map(|ch| {
            if ch.is_control() {
                ch.escape_default().collect::<Vec<_>>()
            } else {
                vec![ch]
            }
        })
        .collect()
}

/// Build and serve a documentation site locally.
pub fn serve(options: ServeOptions) -> Result<(), CommandError> {
    pipeline::runtime()?.block_on(serve_with(
        options,
        crate::execution::ExecutionDeadlines::default(),
        Box::pin(preview::termination()),
    ))
}

/// Options for extracting a portable SQLite snapshot.
#[derive(Debug, Clone)]
pub struct ExtractOptions {
    /// Workspace configuration file.
    pub config: PathBuf,
    /// Explicit destination, or the configuration-relative default.
    pub output: Option<PathBuf>,
}
/// Options for generating a site without source checkouts or execution.
#[derive(Debug, Clone)]
pub struct GenerateOptions {
    /// Completed portable SQLite snapshot.
    pub input: PathBuf,
    /// Destination site directory.
    pub output: PathBuf,
}
/// Execute configured extraction and atomically publish its snapshot.
pub fn extract(options: ExtractOptions) -> Result<(), CommandError> {
    pipeline::runtime()?.block_on(extract_with(
        options,
        crate::execution::ExecutionDeadlines::default(),
        Box::pin(preview::termination()),
    ))?;
    Ok(())
}
/// Generate and atomically publish a site using only a completed snapshot.
pub fn generate(options: GenerateOptions) -> Result<(), CommandError> {
    pipeline::generate_attempt(&options.input, &options.output, true)?;
    Ok(())
}

fn assembly_diagnostics(error: &AssemblyError) -> &[Diagnostic] {
    match error {
        AssemblyError::Diagnostics(diagnostics) => diagnostics,
        AssemblyError::Execution(failure) => &failure.diagnostics,
        AssemblyError::Cleanup { cause, .. } => assembly_diagnostics(cause),
        _ => &[],
    }
}
