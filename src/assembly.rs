//! Assemble explicitly declared sources before reference validation and storage.
//!
//! Loading prepares authored content and resolves API declarations. Rust targets
//! use native compiler tooling, and C++ targets invoke Doxygen. R command
//! adapters generate documentation when configured and discover native APIs.
//! Extraction also completes native semantics and examples before publication.
//! These operations require current command authority from their caller and do
//! not publish a snapshot or site. The resulting IR still
//! needs document-reference, relationship-version, and asset validation before
//! publication; assembly is not a rendering trust token.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::configuration::{ConfigurationError, WorkspaceConfiguration, parse_configuration};
use crate::configuration_validation::validate_configuration_with_source;
use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticPath, DiagnosticSource, Severity};
use crate::documents::{QmdPreparation, includes};
use crate::execution::{
    ExecutionCancellation, ExecutionDeadlines, ExecutionFailure, PageExecutionResult,
};
use crate::ir::{Fingerprint, SourceLocation, Workspace};
use crate::paths::{PathResolutionError, ResolvedWorkspacePaths, resolve_workspace_paths};
use crate::provenance::{
    DeclaredSourceInputs, ProvenanceError, StaticProvenance, collect_static_provenance,
    fingerprint_bytes,
};
use crate::reporting::{Phase, Reporting};

mod adapters;
mod api_examples;
mod authored;
mod documenter;
mod documenter_examples;
mod doxygen_native;
mod julia_environment;
mod julia_runtime;
pub(crate) use julia_runtime::assemble_for_extraction;
#[cfg(target_os = "linux")]
mod execution;
mod package_docs;
mod packages;
mod python_native;
pub(crate) mod r_native;
mod rd_examples;
mod references;
mod reuse;
mod rmarkdown;
mod rust_native;

pub use crate::ir::IncludeSourceRange;
pub(crate) use includes::remap as remap_include_diagnostic;
pub(crate) use includes::watch_inputs as include_watch_inputs;

/// Conventional license inputs shared by package discovery and preview watching.
pub(crate) const PACKAGE_LICENSE_FILENAMES: &[&str] = &[
    "LICENSE",
    "LICENSE.md",
    "LICENCE",
    "LICENCE.md",
    "LICENSE-MIT",
    "LICENSE-APACHE",
    "COPYING",
    "COPYING.md",
];

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

impl From<includes::IncludeError> for AssemblyError {
    fn from(error: includes::IncludeError) -> Self {
        match error {
            includes::IncludeError::Path(error) => Self::Path(error),
            includes::IncludeError::Diagnostics(diagnostics) => Self::Diagnostics(diagnostics),
        }
    }
}

/// One original authored page and its optional QMD preparation.
#[derive(Debug)]
pub struct PreparedPage {
    pub(crate) source_ranges: Vec<IncludeSourceRange>,
    pub(crate) included_resource_targets: std::collections::BTreeSet<String>,
    pub(crate) source: String,
    pub(crate) path: PathBuf,
    pub(crate) relative: DiagnosticPath,
    pub(crate) location: SourceLocation,
    pub(crate) collection: usize,
    pub(crate) preparation: Option<QmdPreparation>,
}
impl PreparedPage {
    /// Original file ranges and include chains for the expanded source.
    pub fn source_ranges(&self) -> &[IncludeSourceRange] {
        &self.source_ranges
    }
    /// Expanded UTF-8 source used for parsing and execution identity.
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
    reporting: Reporting,
    workspace: Workspace,
    configuration: WorkspaceConfiguration,
    configuration_path: PathBuf,
    configuration_source: String,
    paths: ResolvedWorkspacePaths,
    prepared_pages: BTreeMap<String, PreparedPage>,
    package_files: BTreeMap<String, package_docs::PackageFiles>,
    rmd_inputs: BTreeMap<String, Vec<PathBuf>>,
    doxygen_observations: Vec<doxygen_native::DoxygenObservation>,
    r_observations: Vec<r_native::RObservation>,
    cache_observations: CacheObservations,
    python_interpreter: Option<PathBuf>,
    python_doctests: BTreeMap<String, crate::extractors::python::native::DoctestPlans>,
    python_environments: BTreeMap<
        String,
        (
            api_examples::PythonEnvironment,
            crate::incremental::Manifest,
        ),
    >,
    selections: DeclaredSourceInputs,
    evidence: StaticProvenance,
    pub(crate) native_assets: BTreeMap<DiagnosticPath, crate::validation::ContentAsset>,
}

#[derive(Debug, Default)]
struct CacheObservations(Vec<crate::incremental::Manifest>);

impl CacheObservations {
    fn push(&mut self, manifest: crate::incremental::Manifest) {
        // Retain conflicting observations so a later guard still rejects changed inputs.
        if !self.contains(&manifest) {
            self.0.push(manifest);
        }
    }

    fn contains(&self, manifest: &crate::incremental::Manifest) -> bool {
        self.0.contains(manifest)
    }

    fn iter(&self) -> impl Iterator<Item = &crate::incremental::Manifest> {
        self.0.iter()
    }
}

impl Extend<crate::incremental::Manifest> for CacheObservations {
    fn extend<T: IntoIterator<Item = crate::incremental::Manifest>>(&mut self, manifests: T) {
        for manifest in manifests {
            self.push(manifest);
        }
    }
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

    /// Exact selected source files, metadata, and declared environment inputs.
    /// These local paths are used for output protection and preview watching.
    pub fn input_paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.evidence
            .inputs
            .iter()
            .flat_map(|(repository, files)| {
                let root = &self
                    .paths
                    .repositories
                    .iter()
                    .find(|r| &r.id == repository)
                    .expect("observed repository")
                    .path;
                files.keys().map(move |path| root.join(path.as_str()))
            })
            .chain(
                self.r_observations
                    .iter()
                    .flat_map(|observation| observation.paths().cloned()),
            )
            .chain(
                self.cache_observations
                    .iter()
                    .flat_map(|manifest| manifest.paths().cloned()),
            )
    }

    pub(crate) fn doxygen_watch_roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.doxygen_observations
            .iter()
            .flat_map(|observation| observation.roots())
    }

    pub(crate) fn dependency_watch_roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.cache_observations
            .iter()
            .flat_map(|manifest| manifest.roots())
            .chain(
                self.r_observations
                    .iter()
                    .flat_map(|observation| observation.dependency_roots()),
            )
    }

    pub(crate) fn dependency_library_roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.cache_observations
            .iter()
            .flat_map(|manifest| manifest.native_library_roots())
            .chain(
                self.r_observations
                    .iter()
                    .flat_map(|observation| observation.dependency_library_roots()),
            )
    }

    /// Reread selected bytes and containment before accepting a later operation.
    ///
    /// This detects ordinary edits, including a later page changing an earlier
    /// page. It cannot snapshot undeclared reads or changes after this check.
    pub fn revalidate(&self) -> Result<(), AssemblyError> {
        let timing = self.reporting.phase(Phase::SourceVerification, None, None);
        let result = crate::incremental::verification_scope(|| self.revalidate_inner());
        if result.is_ok() {
            timing.finish();
        }
        result
    }

    fn revalidate_inner(&self) -> Result<(), AssemblyError> {
        if self
            .cache_observations
            .iter()
            .any(|manifest| !manifest.verify())
        {
            return Err(AssemblyError::InputsChanged);
        }
        for observation in &self.r_observations {
            observation.verify()?;
        }
        for observation in &self.doxygen_observations {
            observation.verify()?;
        }
        if fs::read(&self.configuration_path)? != self.configuration_source.as_bytes() {
            return Err(AssemblyError::InputsChanged);
        }
        let mut configuration =
            parse_configuration(&self.configuration_source).map_err(|source| {
                ConfigurationError::Parse {
                    path: self.configuration_path.clone(),
                    source,
                }
            })?;
        rmarkdown::collections(&self.configuration_path, &mut configuration)?;
        if configuration != self.configuration {
            return Err(AssemblyError::InputsChanged);
        }
        let paths = resolve_workspace_paths(&self.configuration_path, &self.configuration)?;
        if paths != self.paths {
            return Err(AssemblyError::InputsChanged);
        }
        for (index, collection) in self.configuration.content.iter().enumerate() {
            let content = &paths.content[index];
            if authored::discover(
                &content.path,
                collection.format,
                &paths.repositories[content.repository_index],
            )? != self.selections.content[&collection.id]
            {
                return Err(AssemblyError::InputsChanged);
            }
        }
        if package_docs::discover(&paths, &self.workspace)? != self.package_files {
            return Err(AssemblyError::InputsChanged);
        }
        if rmarkdown::inputs(&paths, &self.configuration)? != self.rmd_inputs {
            return Err(AssemblyError::InputsChanged);
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

/// Read and assemble declared sources using the discovery rules of `check`.
/// Documentation examples do not execute.
/// Rust targets use the prepared compiler and may compile dependencies and run
/// build scripts. C++ targets invoke Doxygen and any configured input filters.
/// Python targets import the prepared checkout and invoke Sphinx autodoc.
/// Julia targets load the verified checkout and reflect on its API and docs.
/// R targets load package code and may regenerate documentation in the checkout.
///
/// The returned workspace is ready for reference validation.
/// Semantic page identity depends on collection ID and relative source path,
/// never a route.
///
/// # Errors
/// Reports malformed declarations, unsafe paths, conflicting fragments,
/// unresolved concepts, source diagnostics, I/O, or changed input observations.
pub fn assemble_workspace(path: impl AsRef<Path>) -> Result<WorkspaceSources, AssemblyError> {
    // Callers may already own a Tokio runtime; a separate thread keeps this
    // synchronous API usable from both synchronous and asynchronous hosts.
    let path = path.as_ref().to_owned();
    std::thread::Builder::new()
        .name("native-documentation-check".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                let mut cancellation: ExecutionCancellation<'_> = Box::pin(std::future::pending());
                assemble_for_check(
                    &path,
                    ExecutionDeadlines::default(),
                    &mut cancellation,
                    &Reporting::default(),
                )
                .await
            })
        })?
        .join()
        .map_err(|_| {
            AssemblyError::Io(std::io::Error::other(
                "Native documentation check thread failed",
            ))
        })?
}

async fn rust_results(
    path: &Path,
    run_examples: bool,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
    reporting: &Reporting,
) -> Result<BTreeMap<(String, String), rust_native::RustNativeExtraction>, AssemblyError> {
    let loading = reporting.stage("Loading workspace", None);
    let configuration_source = fs::read(path)?;
    let configuration = crate::configuration::load_configuration(path)?;
    reject_errors(validate_configuration_with_source(
        &configuration,
        "diplodocus.yaml".try_into().unwrap(),
    ))?;
    let paths = resolve_workspace_paths(path, &configuration)?;
    loading.finish();
    let mut progress = reporting.stage(
        "Extracting Rust APIs",
        Some(target_count(&configuration, "rust")),
    );
    let cache = paths.configuration_directory.join(".diplodocus/cache");
    let mut results = BTreeMap::new();
    for (index, (declaration, package)) in configuration
        .packages
        .iter()
        .zip(&paths.packages)
        .enumerate()
    {
        let repository = &paths.repositories[package.repository_index];
        let result_cache = crate::incremental::package_context(&configuration, &paths, index);
        for (target, selected) in declaration.targets.iter().zip(&package.targets) {
            if target.extractor == "rust" {
                progress.item(&format!("{}:{}", package.id, target.id));
                let result = rust_native::extract_target_cached(
                    repository,
                    package,
                    selected,
                    &target.rust.clone().unwrap_or_default(),
                    &cache,
                    run_examples,
                    reporting,
                    deadlines,
                    cancellation,
                    result_cache.as_ref(),
                )
                .await?;
                results.insert((package.id.clone(), target.id.clone()), result);
                progress.advance();
            }
        }
    }
    if fs::read(path)? != configuration_source {
        return Err(AssemblyError::InputsChanged);
    }
    progress.finish();
    Ok(results)
}

pub(crate) async fn assemble_for_check(
    path: &Path,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
    reporting: &Reporting,
) -> Result<WorkspaceSources, AssemblyError> {
    let rust = rust_results(path, false, deadlines, cancellation, reporting).await?;
    let doxygen = doxygen_results(path, deadlines, cancellation, reporting).await?;
    let r = r_results(
        path,
        deadlines,
        cancellation,
        reporting,
        &mut r_native::GeneratedWrites::default(),
    )
    .await?;
    let python = python_native::results(path, deadlines, cancellation, reporting).await?;
    let julia = julia_runtime::results(path, deadlines, cancellation, reporting).await?;
    assemble_with_native(path, julia, rust, doxygen, r, python, reporting)
}

async fn doxygen_results(
    path: &Path,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
    reporting: &Reporting,
) -> Result<BTreeMap<(String, String), doxygen_native::DoxygenNativeExtraction>, AssemblyError> {
    let before = fs::read(path)?;
    let configuration = crate::configuration::load_configuration(path)?;
    reject_errors(validate_configuration_with_source(
        &configuration,
        "diplodocus.yaml".try_into().unwrap(),
    ))?;
    let paths = resolve_workspace_paths(path, &configuration)?;
    let mut progress = reporting.stage(
        "Extracting C++ APIs",
        Some(target_count(&configuration, "doxygen")),
    );
    let mut results = BTreeMap::new();
    for (index, (declaration, package)) in configuration
        .packages
        .iter()
        .zip(&paths.packages)
        .enumerate()
    {
        let repository = &paths.repositories[package.repository_index];
        let cache = crate::incremental::package_context(&configuration, &paths, index);
        for (target, selected) in declaration.targets.iter().zip(&package.targets) {
            if target.extractor == "doxygen" {
                progress.item(&format!("{}:{}", package.id, target.id));
                results.insert(
                    (package.id.clone(), target.id.clone()),
                    doxygen_native::extract_target(
                        repository,
                        package,
                        selected,
                        reporting,
                        deadlines,
                        cancellation,
                        cache.as_ref(),
                    )
                    .await?,
                );
                progress.advance();
            }
        }
    }
    if fs::read(path)? != before {
        return Err(AssemblyError::InputsChanged);
    }
    progress.finish();
    Ok(results)
}

async fn r_results(
    path: &Path,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
    reporting: &Reporting,
    journal: &mut r_native::GeneratedWrites,
) -> Result<BTreeMap<(String, String), r_native::RNativeExtraction>, AssemblyError> {
    let before = fs::read(path)?;
    let configuration = crate::configuration::load_configuration(path)?;
    reject_errors(validate_configuration_with_source(
        &configuration,
        "diplodocus.yaml".try_into().unwrap(),
    ))?;
    let paths = resolve_workspace_paths(path, &configuration)?;
    let configuration_path = fs::canonicalize(path)?;
    for (declaration, package) in configuration.packages.iter().zip(&paths.packages) {
        if let Some(settings) = &declaration.r {
            for output in &settings.generated_outputs {
                let output = r_native::resolved_output(package, output)?;
                if configuration_path.starts_with(&output)
                    || output.starts_with(&configuration_path)
                    || paths.packages.iter().zip(&configuration.packages).any(
                        |(other, declaration)| {
                            other.id != package.id
                                && (other.path.starts_with(&output)
                                    || (!package.path.starts_with(&other.path)
                                        && output.starts_with(&other.path))
                                    || other.metadata_path.starts_with(&output)
                                    || output.starts_with(&other.metadata_path)
                                    || declaration.r.as_ref().is_some_and(|settings| {
                                        settings
                                            .generated_outputs
                                            .iter()
                                            .chain(&settings.extra_inputs)
                                            .any(|path| {
                                                let path = other.path.join(path);
                                                path.starts_with(&output)
                                                    || output.starts_with(&path)
                                            })
                                    }))
                        },
                    )
                {
                    return Err(AssemblyError::Diagnostics(vec![Diagnostic::new(
                        DiagnosticCode::InvalidConfiguration,
                        Severity::Error,
                        "Generated R outputs must not overlap workspace configuration or another package.",
                    )]));
                }
            }
        }
    }
    let results = r_native::results(
        &configuration,
        &paths,
        deadlines,
        cancellation,
        reporting,
        journal,
    )
    .await?;
    if fs::read(path)? != before {
        return Err(AssemblyError::InputsChanged);
    }
    Ok(results)
}

struct NativeExtractions {
    python: BTreeMap<(String, String), python_native::PythonNativeExtraction>,
    julia: BTreeMap<(String, String), julia_runtime::JuliaNativeExtraction>,
    rust: BTreeMap<(String, String), rust_native::RustNativeExtraction>,
    doxygen: BTreeMap<(String, String), doxygen_native::DoxygenNativeExtraction>,
    r: BTreeMap<(String, String), r_native::RNativeExtraction>,
}

fn assemble_with_native(
    path: &Path,
    julia_results: BTreeMap<(String, String), julia_runtime::JuliaNativeExtraction>,
    rust_results: BTreeMap<(String, String), rust_native::RustNativeExtraction>,
    doxygen_results: BTreeMap<(String, String), doxygen_native::DoxygenNativeExtraction>,
    r_results: BTreeMap<(String, String), r_native::RNativeExtraction>,
    python_results: BTreeMap<(String, String), python_native::PythonNativeExtraction>,
    reporting: &Reporting,
) -> Result<WorkspaceSources, AssemblyError> {
    let text = fs::read_to_string(path).map_err(|source| ConfigurationError::Read {
        path: path.to_owned(),
        source,
    })?;
    let mut configuration =
        parse_configuration(&text).map_err(|source| ConfigurationError::Parse {
            path: path.to_owned(),
            source,
        })?;
    let config_name = DiagnosticPath::try_from(
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("diplodocus.yaml"),
    )
    .map_err(|_| AssemblyError::InputsChanged)?;
    reject_errors(validate_configuration_with_source(
        &configuration,
        config_name.clone(),
    ))?;
    rmarkdown::collections(path, &mut configuration)?;
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
    let mut cache_observations = CacheObservations::default();
    cache_observations.extend(
        rust_results
            .values()
            .filter_map(|result| result.observation.clone()),
    );
    cache_observations.extend(
        python_results
            .values()
            .flat_map(|result| result.observations.iter().cloned()),
    );
    cache_observations.extend(
        julia_results
            .values()
            .flat_map(|result| result.observations.iter().cloned()),
    );
    for package in &configuration.packages {
        cache_observations.push(crate::incremental::supplemental_manifest(
            &configuration,
            &paths,
            package,
        )?);
    }
    for (index, collection) in configuration.content.iter().enumerate() {
        let repository = &paths.repositories[paths.content[index].repository_index];
        let inputs = collection
            .incremental
            .extra_inputs
            .iter()
            .map(|path| crate::incremental::checked_incremental_path(&repository.path, path))
            .collect::<Result<Vec<_>, _>>()?;
        cache_observations.push(crate::incremental::Manifest::capture(
            &inputs,
            &collection.incremental.environment_variables,
        )?);
    }
    let doxygen_observations = doxygen_results
        .values()
        .map(|result| result.observation.clone())
        .collect();
    let r_observations = r_results
        .values()
        .map(|result| result.observation.clone())
        .collect();
    let python_interpreter = python_results
        .values()
        .next()
        .map(|result| result.interpreter.clone());
    let python_environments = python_results
        .iter()
        .filter_map(|((package, _), result)| {
            let environment = result.environment.as_ref()?;
            // Example reuse needs consistent coverage for every target. One
            // target's environment cannot stand in for an opaque loader in another.
            python_results
                .iter()
                .filter(|((owner, _), _)| owner == package)
                .all(|(_, result)| {
                    result
                        .environment
                        .as_ref()
                        .is_some_and(|(candidate, _)| candidate == &environment.0)
                })
                .then(|| (package.clone(), environment.clone()))
        })
        .collect();
    let mut native = NativeExtractions {
        python: python_results,
        julia: julia_results,
        rust: rust_results,
        doxygen: doxygen_results,
        r: r_results,
    };
    let rmd_inputs = rmarkdown::inputs(&paths, &configuration)?;
    for (collection, resolved) in configuration.content.iter().zip(&paths.content) {
        if let Some(files) = rmd_inputs.get(&collection.id) {
            let repository = &paths.repositories[resolved.repository_index];
            selections
                .package_documentation
                .entry(repository.id.clone())
                .or_default()
                .extend(files.iter().cloned());
            for file in files {
                let path = repository.path.join(file);
                observe(
                    &mut observed,
                    &repository.source_location(&path, None)?,
                    fingerprint_bytes(&fs::read(path)?),
                )?;
            }
        }
    }
    let python_doctests = packages::assemble(
        &configuration,
        &paths,
        &mut workspace,
        &mut selections,
        &mut observed,
        &mut native,
        reporting,
    )?;
    let prepared_pages = authored::assemble(
        &configuration,
        &paths,
        &mut workspace,
        &mut selections,
        &mut observed,
        reporting,
    )?;
    documenter::assemble(&mut workspace, &prepared_pages);
    let package_files = package_docs::assemble(
        &paths,
        &mut workspace,
        &prepared_pages,
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
        reporting: reporting.clone(),
        workspace,
        configuration,
        // Configuration-relative declarations retain the caller's directory,
        // even when the configuration file itself is a symlink elsewhere.
        configuration_path: std::env::current_dir()?.join(path),
        configuration_source: text,
        paths,
        prepared_pages,
        package_files,
        rmd_inputs,
        doxygen_observations,
        r_observations,
        cache_observations,
        python_interpreter,
        python_doctests,
        python_environments,
        selections,
        evidence,
        native_assets: BTreeMap::new(),
    };
    result.revalidate()?;
    Ok(result)
}

fn target_count(configuration: &WorkspaceConfiguration, extractor: &str) -> usize {
    configuration
        .packages
        .iter()
        .flat_map(|package| &package.targets)
        .filter(|target| target.extractor == extractor)
        .count()
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
