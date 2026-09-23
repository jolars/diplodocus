//! Assemble explicitly declared sources before reference validation and storage.
//!
//! Loading performs static extraction and authored preparation only. The separate
//! consuming execution operation requires current command authority from its
//! caller. Neither operation publishes a snapshot or site. The resulting IR still
//! needs document-reference, relationship-version, and asset validation before
//! publication; assembly is not a rendering trust token.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::configuration::{ConfigurationError, WorkspaceConfiguration, parse_configuration};
use crate::configuration_validation::validate_configuration_with_source;
use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticPath, DiagnosticSource, Severity};
use crate::documents::QmdPreparation;
use crate::execution::{
    ExecutionCancellation, ExecutionDeadlines, ExecutionFailure, PageExecutionResult,
};
use crate::ir::{Fingerprint, SourceLocation, Workspace};
use crate::paths::{PathResolutionError, ResolvedWorkspacePaths, resolve_workspace_paths};
use crate::provenance::{
    DeclaredSourceInputs, ProvenanceError, StaticProvenance, collect_static_provenance,
    fingerprint_bytes,
};

mod authored;
#[cfg(target_os = "linux")]
mod execution;
mod packages;
mod references;

/// A failed source assembly or execution, without a publishable workspace.
#[derive(Debug, thiserror::Error)]
pub enum AssemblyError {
    /// Configuration parsing failed.
    #[error(transparent)]
    Configuration(#[from] ConfigurationError),
    /// Declared paths failed their boundary checks.
    #[error(transparent)]
    Path(#[from] PathResolutionError),
    /// Static source observations failed.
    #[error(transparent)]
    Provenance(#[from] ProvenanceError),
    /// Independent source errors in deterministic diagnostic order.
    #[error("workspace assembly failed with source diagnostics")]
    Diagnostics(Vec<Diagnostic>),
    /// File reading or temporary staging failed.
    #[error("workspace source or staging I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// A consumed input no longer agrees with the assembled evidence.
    #[error("workspace inputs changed during assembly or execution")]
    InputsChanged,
    /// The caller canceled the workspace operation between active page sessions.
    #[error("workspace execution was canceled")]
    Cancelled,
    /// Authored execution failed after supervised kernel cleanup.
    #[error(transparent)]
    Execution(#[from] ExecutionFailure),
    /// Cleanup failed after another failure; preserve both causes.
    #[error("{cause}; workspace staging cleanup also failed: {source}")]
    Cleanup {
        /// Original failure.
        cause: Box<AssemblyError>,
        /// Staging cleanup error.
        source: std::io::Error,
    },
}

/// One original authored page and its optional QMD preparation.
#[derive(Debug)]
pub struct PreparedPage {
    pub(crate) source: String,
    pub(crate) path: PathBuf,
    pub(crate) relative: DiagnosticPath,
    pub(crate) location: SourceLocation,
    pub(crate) collection: usize,
    pub(crate) preparation: Option<QmdPreparation>,
}
impl PreparedPage {
    /// Exact original UTF-8 source used for parsing and execution identity.
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Pure authored preparation, including disabled cells and page vetoes.
    pub fn preparation(&self) -> Option<&QmdPreparation> {
        self.preparation.as_ref()
    }
}

/// Static source assembly; local paths and original inputs never serialize.
#[derive(Debug)]
pub struct WorkspaceSources {
    workspace: Workspace,
    configuration: WorkspaceConfiguration,
    configuration_path: PathBuf,
    configuration_source: String,
    paths: ResolvedWorkspacePaths,
    prepared_pages: BTreeMap<String, PreparedPage>,
    selections: DeclaredSourceInputs,
    evidence: StaticProvenance,
}
impl WorkspaceSources {
    /// Portable records assembled so far, without a claim of final validation.
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }
    /// Original authored inputs keyed by stable page identity.
    pub fn prepared_pages(&self) -> &BTreeMap<String, PreparedPage> {
        &self.prepared_pages
    }
    /// Explicit declarations consumed by this assembly.
    pub fn configuration(&self) -> &WorkspaceConfiguration {
        &self.configuration
    }
    /// Runtime boundaries for later declared-input watching and asset collection.
    pub fn paths(&self) -> &ResolvedWorkspacePaths {
        &self.paths
    }

    /// Reread selected bytes and containment before accepting a later operation.
    ///
    /// This detects ordinary edits, including a later page changing an earlier
    /// page. It cannot snapshot undeclared reads or changes after this check.
    pub fn revalidate(&self) -> Result<(), AssemblyError> {
        if fs::read(&self.configuration_path)? != self.configuration_source.as_bytes() {
            return Err(AssemblyError::InputsChanged);
        }
        let paths = resolve_workspace_paths(&self.configuration_path, &self.configuration)?;
        if paths != self.paths {
            return Err(AssemblyError::InputsChanged);
        }
        for (index, collection) in self.configuration.content.iter().enumerate() {
            if authored::discover(&paths.content[index].path, collection.format)?
                != self.selections.content[&collection.id]
            {
                return Err(AssemblyError::InputsChanged);
            }
        }
        let current = collect_static_provenance(
            &self.configuration_path,
            &self.configuration,
            &self.selections,
        )?;
        if current.inputs != self.evidence.inputs {
            return Err(AssemblyError::InputsChanged);
        }
        Ok(())
    }
}

/// Local settings for an explicitly authorized workspace execution attempt.
pub struct WorkspaceExecution<'a> {
    /// Existing directory beneath which the attempt owns temporary staging.
    pub staging_parent: &'a Path,
    /// Phase limits shared by this attempt's page sessions.
    pub deadlines: ExecutionDeadlines,
    /// Cooperative cancellation, also observed between pages.
    pub cancellation: ExecutionCancellation<'a>,
}

/// Executed workspace and its private staging lifetime.
///
/// Each page retains its validated wrappers alongside the portable IR. Dropping
/// this owner removes the attempt's staging, including assets of earlier pages.
/// Consumers must copy verified bytes into their own transaction before disposal.
#[derive(Debug)]
pub struct ExecutedWorkspace {
    sources: WorkspaceSources,
    pages: BTreeMap<String, PageExecutionResult>,
    staging: Option<tempfile::TempDir>,
}
impl ExecutedWorkspace {
    /// Portable records with complete executed-cell outputs and provenance.
    pub fn workspace(&self) -> &Workspace {
        self.sources.workspace()
    }
    /// Current page results with active validation evidence and retained files.
    pub fn executed_pages(&self) -> &BTreeMap<String, PageExecutionResult> {
        &self.pages
    }
    /// Original declarations, source preparations, and runtime boundaries.
    pub fn sources(&self) -> &WorkspaceSources {
        &self.sources
    }
    /// Explicitly discard staging and report filesystem cleanup failure.
    pub fn discard(mut self) -> Result<(), AssemblyError> {
        self.pages.clear();
        if let Some(staging) = self.staging.take() {
            staging.close()?;
        }
        Ok(())
    }
}

/// Read and assemble declared static sources without executing authored code.
///
/// All packages and pages are inspected before errors are returned. No kernel,
/// cache, staging directory, snapshot, or output site is created. Semantic page
/// identity depends on collection ID and relative source path, never a route.
///
/// # Errors
/// Reports malformed declarations, unsafe paths, conflicting fragments,
/// unresolved concepts, source diagnostics, I/O, or changed input observations.
pub fn assemble_workspace(path: impl AsRef<Path>) -> Result<WorkspaceSources, AssemblyError> {
    let path = path.as_ref();
    let text = fs::read_to_string(path).map_err(|source| ConfigurationError::Read {
        path: path.to_owned(),
        source,
    })?;
    let configuration = parse_configuration(&text).map_err(|source| ConfigurationError::Parse {
        path: path.to_owned(),
        source,
    })?;
    let config_name = DiagnosticPath::try_from(
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("diplodocus.toml"),
    )
    .map_err(|_| AssemblyError::InputsChanged)?;
    reject_errors(validate_configuration_with_source(
        &configuration,
        config_name.clone(),
    ))?;
    let paths = resolve_workspace_paths(path, &configuration)?;
    let mut workspace = Workspace {
        name: configuration.project.name.clone(),
        ..Workspace::default()
    };
    let mut selections = DeclaredSourceInputs::default();
    let mut observed = BTreeMap::new();
    packages::assemble(
        &configuration,
        &paths,
        &mut workspace,
        &mut selections,
        &mut observed,
    )?;
    let prepared_pages = authored::assemble(
        &configuration,
        &paths,
        &mut workspace,
        &mut selections,
        &mut observed,
    )?;
    references::concepts_and_relationships(&configuration, &mut workspace, &config_name);
    reject_errors(workspace.diagnostics.iter().cloned().collect())?;
    let evidence = collect_static_provenance(path, &configuration, &selections)?;
    for ((repository, path), fingerprint) in observed {
        if evidence
            .inputs
            .get(&repository)
            .and_then(|inputs| inputs.get(&path))
            != Some(&fingerprint)
        {
            return Err(AssemblyError::InputsChanged);
        }
    }
    workspace.repositories = evidence.repositories.clone();
    let result = WorkspaceSources {
        workspace,
        configuration,
        // Configuration-relative declarations retain the caller's directory,
        // even when the configuration file itself is a symlink elsewhere.
        configuration_path: std::env::current_dir()?.join(path),
        configuration_source: text,
        paths,
        prepared_pages,
        selections,
        evidence,
    };
    result.revalidate()?;
    Ok(result)
}

type ObservedInputs = BTreeMap<(String, DiagnosticPath), Fingerprint>;

fn observe(
    inputs: &mut ObservedInputs,
    source: &SourceLocation,
    fingerprint: Fingerprint,
) -> Result<(), AssemblyError> {
    if inputs
        .insert(
            (source.repository.clone(), source.path.clone()),
            fingerprint.clone(),
        )
        .is_some_and(|old| old != fingerprint)
    {
        return Err(AssemblyError::InputsChanged);
    }
    Ok(())
}

fn reject_errors(diagnostics: Vec<Diagnostic>) -> Result<(), AssemblyError> {
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        Err(AssemblyError::Diagnostics(diagnostics))
    } else {
        Ok(())
    }
}

fn diagnostic(code: DiagnosticCode, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(code, Severity::Error, message)
}

fn portable(path: &Path) -> Result<DiagnosticPath, AssemblyError> {
    DiagnosticPath::try_from(path.to_str().ok_or(AssemblyError::InputsChanged)?)
        .map_err(|_| AssemblyError::InputsChanged)
}

fn relative(path: &Path, base: &Path) -> Result<Option<DiagnosticPath>, AssemblyError> {
    let relative = path
        .strip_prefix(base)
        .map_err(|_| AssemblyError::InputsChanged)?;
    if relative.as_os_str().is_empty() {
        Ok(None)
    } else {
        portable(relative).map(Some)
    }
}

fn source(source: &SourceLocation) -> DiagnosticSource {
    DiagnosticSource::Repository {
        repository: source.repository.clone(),
        path: source.path.clone(),
    }
}
