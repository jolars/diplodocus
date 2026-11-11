use super::*;
use std::fs;
use std::path::{Component, Path};

use crate::assembly::WorkspaceExecution;
use crate::execution::{ExecutionCancellation, ExecutionDeadlines};
use crate::rendering::RenderedSite;
use crate::reporting::{Event, Reporting};
use crate::site::Site;
use crate::snapshots::Snapshot;
use crate::validation::resolve_executed_workspace;

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
    reporting.run("Validating documentation", || {
        let resolved = resolve_workspace(&sources)?;
        let mut diagnostics: Vec<_> = resolved.diagnostics().iter().cloned().collect();
        diagnostics.extend(Site::check_presentation(&sources, &resolved)?);
        diagnostics.sort();
        Ok(CheckReport { diagnostics })
    })
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
    mut cancellation: ExecutionCancellation<'_>,
    reporting: &Reporting,
) -> Result<PathBuf, CommandError> {
    let mut sources = crate::assembly::assemble_for_extraction(
        &options.config,
        deadlines,
        &mut cancellation,
        reporting,
    )
    .await?;
    let resolved = reporting.run("Validating documentation", || resolve_workspace(&sources))?;
    let output = options.output.unwrap_or_else(|| {
        sources
            .paths()
            .configuration_directory
            .join(".diplodocus/documentation.sqlite")
    });
    protect_sources(&sources, &resolved, &options.config, &output)?;
    if let Some(site) = site_output {
        protect_sources(&sources, &resolved, &options.config, site)?;
        reject_overlap(site, &output)?;
    }
    sources
        .complete_package_metadata(deadlines, &mut cancellation)
        .await?;
    sources
        .execute_api_examples(deadlines, &mut cancellation)
        .await?;
    sources
        .execute_rd_examples(deadlines, &mut cancellation)
        .await?;
    sources
        .execute_rmarkdown(deadlines, &mut cancellation)
        .await?;
    sources
        .execute_documenter_examples(deadlines, &mut cancellation)
        .await?;
    #[cfg(target_os = "linux")]
    let snapshot = {
        // Execution staging stays outside watched source trees and survives until copying finishes.
        let staging = tempfile::tempdir()?;
        let executed = sources
            .execute(WorkspaceExecution {
                staging_parent: staging.path(),
                deadlines,
                cancellation,
            })
            .await?;
        let resolved = reporting.run("Validating executed documentation", || {
            resolve_executed_workspace(&executed)
        })?;
        protect_sources(executed.sources(), &resolved, &options.config, &output)?;
        if let Some(site) = site_output {
            protect_sources(executed.sources(), &resolved, &options.config, site)?;
        }
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
    reporting.run("Publishing snapshot", || snapshot.publish(&output))?;
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
    let input = extract_attempt(
        ExtractOptions {
            config: options.config.clone(),
            output: None,
        },
        Some(&options.output),
        deadlines,
        cancellation,
        reporting,
    )
    .await?;
    generate_attempt(&input, &options.output, false, true, reporting)
}
pub(super) fn generate_attempt(
    input: &Path,
    output: &Path,
    report_diagnostics: bool,
    allow_components: bool,
    reporting: &Reporting,
) -> Result<RenderedSite, CommandError> {
    reject_overlap(output, input)?;
    let snapshot = reporting.run("Loading snapshot", || Snapshot::load(input))?;
    if report_diagnostics {
        for diagnostic in &snapshot.workspace().diagnostics {
            reporting.emit(Event::Diagnostic(diagnostic));
        }
    }
    let site = reporting
        .run("Preparing site", || Site::new(&snapshot))?
        .with_components(allow_components);
    let rendered = crate::rendering::render_reported(&site, true, reporting)?;
    rendered.publish_reported(output, reporting)?;
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
    let mut site = reporting.run("Preparing site", || Site::new(&snapshot))?;
    if let Some(path) = map {
        reject_overlap(output, path)?;
        let urls = serde_json::from_slice(&fs::read(path)?)
            .map_err(|error| crate::site::SiteError::Route(format!("invalid URL map: {error}")))?;
        site = site.with_url_map(urls)?;
    }
    let rendered = crate::rendering::render_reported(&site, false, reporting)?;
    rendered.publish_reported(output, reporting)?;
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
    // The cache rejects symlinked roots itself. Compare its reserved location
    // without inspecting cache storage on disabled execution paths.
    let destination = absolute(output)?;
    for kind in ["execution", "rust"] {
        let cache = sources
            .paths()
            .configuration_directory
            .join(".diplodocus/cache")
            .join(kind);
        if destination.starts_with(&cache) || cache.starts_with(&destination) {
            return Err(CommandError::InputOverlap);
        }
    }
    for input in sources
        .input_paths()
        .chain(resolved.input_paths().map(Path::to_owned))
    {
        reject_overlap(output, &input)?;
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
