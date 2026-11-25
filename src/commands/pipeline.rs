use super::*;
use std::fs;
use std::path::{Component, Path};

use crate::assembly::{WorkspaceExecution, WorkspaceSources};
use crate::execution::{ExecutionCancellation, ExecutionDeadlines};
use crate::rendering::RenderedSite;
use crate::reporting::{Event, Phase, Reporting};
use crate::site::Site;
use crate::snapshots::Snapshot;
use crate::validation::resolve_executed_workspace;

#[derive(Default)]
pub(super) struct AttemptState {
    pub writes: crate::assembly::r_native::GeneratedWrites,
    pub watch_inputs: Option<super::watching::SourceWatchInputs>,
    pub watch_sources: bool,
}

impl AttemptState {
    pub fn watching() -> Self {
        Self {
            watch_sources: true,
            ..Self::default()
        }
    }
}

pub(super) fn runtime() -> Result<tokio::runtime::Runtime, CommandError> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?)
}

pub(super) fn check_attempt(
    options: CheckOptions,
    reporting: &Reporting,
) -> Result<CheckReport, CommandError> {
    let sources = runtime()?.block_on(async {
        let mut cancellation: ExecutionCancellation<'_> = Box::pin(super::watching::termination());
        crate::assembly::assemble_for_check(
            &options.config,
            ExecutionDeadlines::default(),
            &mut cancellation,
            reporting,
        )
        .await
    })?;
    reporting.run_phase(
        Phase::ReferenceResolution,
        "Validating documentation",
        || {
            let resolved = resolve_workspace(&sources)?;
            let mut diagnostics: Vec<_> = resolved.diagnostics().iter().cloned().collect();
            diagnostics.extend(Site::check_presentation(&sources, &resolved)?);
            diagnostics.sort();
            Ok(CheckReport { diagnostics })
        },
    )
}

/// Extract a snapshot with explicit execution limits and cooperative cancellation.
/// Cancellation is awaited through kernel cleanup before returning an error.
pub async fn extract_with(
    options: ExtractOptions,
    deadlines: ExecutionDeadlines,
    cancellation: ExecutionCancellation<'_>,
) -> Result<PathBuf, CommandError> {
    CommandRunner::default()
        .extract_with(options, deadlines, cancellation)
        .await
}

pub(super) async fn extract_attempt(
    options: ExtractOptions,
    site_output: Option<&Path>,
    deadlines: ExecutionDeadlines,
    cancellation: ExecutionCancellation<'_>,
    reporting: &Reporting,
) -> Result<PathBuf, CommandError> {
    extract_attempt_observed(
        options,
        site_output,
        deadlines,
        cancellation,
        reporting,
        &mut AttemptState::default(),
    )
    .await
}

pub(super) async fn extract_attempt_observed(
    options: ExtractOptions,
    site_output: Option<&Path>,
    deadlines: ExecutionDeadlines,
    cancellation: ExecutionCancellation<'_>,
    reporting: &Reporting,
    attempt: &mut AttemptState,
) -> Result<PathBuf, CommandError> {
    let mut outputs = Vec::new();
    if let Some(site) = site_output {
        outputs.push(absolute(site)?);
    }
    if let Some(output) = &options.output {
        outputs.push(absolute(output)?);
    }
    let parent = options
        .config
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let configuration_directory = fs::canonicalize(parent)?;
    for destination in &outputs {
        protect_caches(&configuration_directory, destination)?;
    }
    crate::incremental::scope_outputs(
        outputs,
        extract_attempt_observed_inner(
            options,
            site_output,
            deadlines,
            cancellation,
            reporting,
            attempt,
        ),
    )
    .await
}

async fn extract_attempt_observed_inner(
    options: ExtractOptions,
    site_output: Option<&Path>,
    deadlines: ExecutionDeadlines,
    mut cancellation: ExecutionCancellation<'_>,
    reporting: &Reporting,
    attempt: &mut AttemptState,
) -> Result<PathBuf, CommandError> {
    let mut sources = crate::assembly::assemble_for_extraction(
        &options.config,
        deadlines,
        &mut cancellation,
        reporting,
        &mut attempt.writes,
        false,
    )
    .await?;
    let output = options.output.unwrap_or_else(|| {
        sources
            .paths()
            .configuration_directory
            .join(".diplodocus/documentation.sqlite")
    });
    if attempt.watch_sources {
        attempt.watch_inputs = Some(super::watching::SourceWatchInputs::capture(
            &sources,
            &absolute(site_output.unwrap_or(&output))?,
            reporting,
            attempt.watch_inputs.take(),
        ));
    }
    reporting.run("Validating documentation", || {
        let timing = reporting.phase(Phase::ReferenceResolution, None, None);
        let resolved = resolve_workspace(&sources)?;
        timing.finish();
        protect_sources(&sources, &resolved, &options.config, &output)?;
        if let Some(site) = site_output {
            protect_sources(&sources, &resolved, &options.config, site)?;
            reject_overlap(site, &output)?;
        }
        Ok::<_, CommandError>(())
    })?;
    complete_api(&mut sources, deadlines, &mut cancellation).await?;
    sources
        .execute_rmarkdown(deadlines, &mut cancellation)
        .await?;
    sources
        .execute_documenter_examples(deadlines, &mut cancellation)
        .await?;
    #[cfg(target_os = "linux")]
    let snapshot = {
        // Execution staging can live inside a source tree when TMPDIR is set.
        let staging = tempfile::Builder::new()
            .prefix(crate::generated_storage::EXECUTION_TEMP_PREFIX)
            .tempdir()?;
        let executed = sources
            .execute(WorkspaceExecution {
                staging_parent: staging.path(),
                deadlines,
                cancellation,
            })
            .await?;
        if attempt.watch_sources {
            attempt.watch_inputs = Some(super::watching::SourceWatchInputs::capture(
                executed.sources(),
                &absolute(site_output.unwrap_or(&output))?,
                reporting,
                attempt.watch_inputs.take(),
            ));
        }
        let resolved = reporting.run("Validating executed documentation", || {
            let timing = reporting.phase(Phase::ReferenceResolution, None, None);
            let resolved = resolve_executed_workspace(&executed)?;
            timing.finish();
            protect_sources(executed.sources(), &resolved, &options.config, &output)?;
            if let Some(site) = site_output {
                protect_sources(executed.sources(), &resolved, &options.config, site)?;
            }
            Ok::<_, CommandError>(resolved)
        })?;
        let snapshot = reporting.run("Preparing snapshot", || {
            Snapshot::from_executed(&executed, &resolved)
        })?;
        executed.discard()?;
        snapshot
    };
    #[cfg(not(target_os = "linux"))]
    let snapshot = {
        let _ = (deadlines, cancellation);
        if sources
            .prepared_pages()
            .values()
            .any(|p| p.preparation().is_some_and(|p| p.execution_eligible))
        {
            return Err(CommandError::UnsupportedExecution);
        }
        let resolved = resolve_workspace(&sources)?;
        reporting.run("Preparing snapshot", || {
            Snapshot::from_sources(&sources, &resolved)
        })?
    };
    reporting.run_phase(Phase::Publication, "Publishing snapshot", || {
        snapshot.publish(&output)
    })?;
    for diagnostic in &snapshot.workspace().diagnostics {
        reporting.emit(Event::Diagnostic(diagnostic));
    }
    Ok(output)
}

pub(super) async fn build_attempt(
    options: &BuildOptions,
    deadlines: ExecutionDeadlines,
    cancellation: ExecutionCancellation<'_>,
    reporting: &Reporting,
) -> Result<RenderedSite, CommandError> {
    build_attempt_observed(
        options,
        deadlines,
        cancellation,
        reporting,
        &mut AttemptState::default(),
        SiteMode::Publish,
    )
    .await
}

#[derive(Clone, Copy)]
pub(super) enum SiteMode {
    Publish,
    Preview,
}

pub(super) async fn build_attempt_observed(
    options: &BuildOptions,
    deadlines: ExecutionDeadlines,
    cancellation: ExecutionCancellation<'_>,
    reporting: &Reporting,
    attempt: &mut AttemptState,
    mode: SiteMode,
) -> Result<RenderedSite, CommandError> {
    let input = extract_attempt_observed(
        ExtractOptions {
            config: options.config.clone(),
            output: None,
        },
        Some(&options.output),
        deadlines,
        cancellation,
        reporting,
        attempt,
    )
    .await?;
    generate_attempt(
        &input,
        &options.output,
        false,
        super::GenerateTrust {
            allow_components: true,
            allow_site_scripts: true,
        },
        mode,
        reporting,
    )
}
pub(super) fn generate_attempt(
    input: &Path,
    output: &Path,
    report_diagnostics: bool,
    trust: super::GenerateTrust,
    mode: SiteMode,
    reporting: &Reporting,
) -> Result<RenderedSite, CommandError> {
    reject_overlap(output, input)?;
    let snapshot = reporting.run("Loading snapshot", || Snapshot::load(input))?;
    if report_diagnostics {
        for diagnostic in &snapshot.workspace().diagnostics {
            reporting.emit(Event::Diagnostic(diagnostic));
        }
    }
    let mut site = reporting
        .run_phase(Phase::SitePreparation, "Preparing site", || {
            Site::new(&snapshot)
        })?
        .with_components(trust.allow_components)
        .with_site_scripts(trust.allow_site_scripts);
    site.preview = matches!(mode, SiteMode::Preview);
    let rendering = reporting.phase(Phase::Rendering, None, None);
    let rendered = crate::rendering::render_reported(&site, true, reporting)?;
    rendering.finish();
    let publication = reporting.phase(Phase::Publication, None, None);
    rendered.publish_reported(output, reporting)?;
    publication.finish();
    Ok(rendered)
}

pub(super) fn generate_fragments_attempt(
    input: &Path,
    output: &Path,
    map: Option<&Path>,
    reporting: &Reporting,
) -> Result<RenderedSite, CommandError> {
    reject_overlap(output, input)?;
    let snapshot = reporting.run("Loading snapshot", || Snapshot::load(input))?;
    let mut site = reporting.run_phase(Phase::SitePreparation, "Preparing site", || {
        Site::new(&snapshot)
    })?;
    if let Some(path) = map {
        reject_overlap(output, path)?;
        let urls = serde_json::from_slice(&fs::read(path)?)
            .map_err(|error| crate::site::SiteError::Route(format!("invalid URL map: {error}")))?;
        site = site.with_url_map(urls)?;
    }
    let rendering = reporting.phase(Phase::Rendering, None, None);
    let rendered = crate::rendering::render_reported(&site, false, reporting)?;
    rendering.finish();
    let publication = reporting.phase(Phase::Publication, None, None);
    rendered.publish_reported(output, reporting)?;
    publication.finish();
    Ok(rendered)
}

/// Resolve existing ancestors, including symlink aliases, without creating a path.
pub(super) fn absolute(path: &Path) -> Result<PathBuf, CommandError> {
    let absolute = std::env::current_dir()?.join(path);
    let mut current = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::RootDir | Component::Prefix(_) => current.push(part.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                current.pop();
            }
            Component::Normal(name) => {
                current.push(name);
                match fs::symlink_metadata(&current) {
                    Ok(_) => current = fs::canonicalize(&current)?,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
    }
    Ok(current)
}
pub(super) fn reject_overlap(output: &Path, input: &Path) -> Result<(), CommandError> {
    let output = absolute(output)?;
    let input = absolute(input)?;
    if input.starts_with(&output) || output == input {
        return Err(CommandError::InputOverlap);
    }
    Ok(())
}
fn protect_sources(
    sources: &crate::assembly::WorkspaceSources,
    resolved: &crate::validation::ResolvedWorkspace,
    config: &Path,
    output: &Path,
) -> Result<(), CommandError> {
    reject_overlap(output, config)?;
    let destination = absolute(output)?;
    protect_caches(&sources.paths().configuration_directory, &destination)?;
    for directory in resolved.public_directory_paths() {
        if destination.starts_with(directory) {
            return Err(CommandError::InputOverlap);
        }
    }

    for input in sources
        .input_paths()
        .chain(resolved.input_paths().map(Path::to_owned))
    {
        reject_overlap(output, &input)?;
    }
    for (declaration, package) in sources
        .configuration()
        .packages
        .iter()
        .zip(&sources.paths().packages)
    {
        if declaration.ecosystem == "r"
            && crate::assembly::r_native::native_source_path(&package.path, &destination)
        {
            return Err(crate::assembly::AssemblyError::Diagnostics(vec![
                crate::diagnostics::Diagnostic::new(
                    crate::diagnostics::DiagnosticCode::RMetadata,
                    crate::diagnostics::Severity::Error,
                    "Output must be outside native R input trees. Keep snapshots in a location such as `.diplodocus/documentation.sqlite`.",
                ),
            ]).into());
        }
    }
    // Reject replacement of a discovery root even if it is currently empty.
    for root in sources.paths().content.iter().map(|c| &c.path).chain(
        sources
            .paths()
            .packages
            .iter()
            .flat_map(|p| p.targets.iter().map(|t| &t.path)),
    ) {
        reject_overlap(output, root)?;
    }
    Ok(())
}

fn protect_caches(configuration_directory: &Path, destination: &Path) -> Result<(), CommandError> {
    // Compare reserved locations without inspecting disabled cache storage.
    for kind in ["execution", "rust", "incremental", "package-bundles"] {
        let cache = configuration_directory.join(".diplodocus/cache").join(kind);
        if destination.starts_with(&cache) || cache.starts_with(destination) {
            return Err(CommandError::InputOverlap);
        }
    }
    Ok(())
}

async fn complete_api(
    sources: &mut WorkspaceSources,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
) -> Result<(), CommandError> {
    sources
        .complete_package_metadata(deadlines, cancellation)
        .await?;
    sources
        .execute_api_examples(deadlines, cancellation)
        .await?;
    sources.execute_rd_examples(deadlines, cancellation).await?;
    Ok(())
}

pub(super) async fn export_bundle(
    options: BundleExportOptions,
    mut cancellation: ExecutionCancellation<'_>,
    reporting: &Reporting,
) -> Result<(), CommandError> {
    let destination = absolute(&options.output)?;
    reject_overlap(&options.output, &options.config)?;
    let parent = options
        .config
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    protect_caches(&fs::canonicalize(parent)?, &destination)?;
    let deadlines = ExecutionDeadlines::default();
    crate::incremental::scope_outputs(vec![destination], async {
        let mut sources = crate::assembly::assemble_for_extraction(
            &options.config,
            deadlines,
            &mut cancellation,
            reporting,
            &mut Default::default(),
            true,
        )
        .await?;
        let resolved = crate::validation::resolve_bundle(&sources)?;
        protect_sources(&sources, &resolved, &options.config, &options.output)?;
        complete_api(&mut sources, deadlines, &mut cancellation).await?;
        let resolved = crate::validation::resolve_bundle(&sources)?;
        protect_sources(&sources, &resolved, &options.config, &options.output)?;
        let bundle = Snapshot::from_bundle(&sources, &resolved)?;
        reporting.run_phase(Phase::Publication, "Publishing package bundle", || {
            bundle.publish(&options.output)
        })?;
        for diagnostic in &bundle.workspace().diagnostics {
            reporting.emit(Event::Diagnostic(diagnostic));
        }
        Ok(())
    })
    .await
}
