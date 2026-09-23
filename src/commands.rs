//! Testable operations invoked by the command-line interface.

use std::net::IpAddr;
use std::path::PathBuf;

use thiserror::Error;

use crate::assembly::{AssemblyError, assemble_workspace};
use crate::diagnostics::{Diagnostic, DiagnosticSource, Severity};
use crate::validation::{ResolutionError, resolve_workspace};

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
    /// Building is not available in the infrastructure milestone.
    #[error("`diplodocus build` is not implemented yet")]
    BuildNotImplemented,
    /// Static source assembly failed.
    #[error(transparent)]
    Assembly(#[from] AssemblyError),
    /// Semantic or local reference validation failed.
    #[error(transparent)]
    Resolution(#[from] ResolutionError),
    /// Serving is not available in the infrastructure milestone.
    #[error("`diplodocus serve` is not implemented yet")]
    ServeNotImplemented,
}

impl CommandError {
    /// Portable source diagnostics when the operation reached source validation.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::Assembly(AssemblyError::Diagnostics(diagnostics)) => diagnostics,
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
pub fn build(_options: BuildOptions) -> Result<(), CommandError> {
    Err(CommandError::BuildNotImplemented)
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
pub fn serve(_options: ServeOptions) -> Result<(), CommandError> {
    Err(CommandError::ServeNotImplemented)
}
