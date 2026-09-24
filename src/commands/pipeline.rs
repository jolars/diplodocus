use super::*;
use std::fs;
use std::path::{Component, Path};

use crate::assembly::WorkspaceExecution;
use crate::execution::{ExecutionCancellation, ExecutionDeadlines};
use crate::rendering::{RenderedSite, render_site};
use crate::site::Site;
use crate::snapshots::Snapshot;
use crate::validation::resolve_executed_workspace;

pub(super) fn runtime() -> Result<tokio::runtime::Runtime, CommandError> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?)
}

/// Extract a snapshot with explicit execution limits and cooperative cancellation.
/// Cancellation is awaited through kernel cleanup before returning an error.
pub async fn extract_with(
    options: ExtractOptions,
    deadlines: ExecutionDeadlines,
    cancellation: ExecutionCancellation<'_>,
) -> Result<PathBuf, CommandError> {
    extract_attempt(options, None, deadlines, cancellation).await
}

pub(super) async fn extract_attempt(
    options: ExtractOptions,
    site_output: Option<&Path>,
    deadlines: ExecutionDeadlines,
    cancellation: ExecutionCancellation<'_>,
) -> Result<PathBuf, CommandError> {
    let sources = assemble_workspace(&options.config)?;
    let resolved = resolve_workspace(&sources)?;
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
        let resolved = resolve_executed_workspace(&executed)?;
        protect_sources(executed.sources(), &resolved, &options.config, &output)?;
        if let Some(site) = site_output {
            protect_sources(executed.sources(), &resolved, &options.config, site)?;
        }
        let snapshot = Snapshot::from_executed(&executed, &resolved)?;
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
        Snapshot::from_sources(&sources, &resolved)?
    };
    snapshot.publish(&output)?;
    for diagnostic in &snapshot.workspace().diagnostics {
        eprintln!("{}", format_diagnostic(diagnostic));
    }
    Ok(output)
}

pub(super) async fn build_attempt(
    options: &BuildOptions,
    deadlines: ExecutionDeadlines,
    cancellation: ExecutionCancellation<'_>,
) -> Result<RenderedSite, CommandError> {
    let input = extract_attempt(
        ExtractOptions {
            config: options.config.clone(),
            output: None,
        },
        Some(&options.output),
        deadlines,
        cancellation,
    )
    .await?;
    generate_attempt(&input, &options.output, false)
}
pub(super) fn generate_attempt(
    input: &Path,
    output: &Path,
    report_diagnostics: bool,
) -> Result<RenderedSite, CommandError> {
    reject_overlap(output, input)?;
    let snapshot = Snapshot::load(input)?;
    if report_diagnostics {
        for diagnostic in &snapshot.workspace().diagnostics {
            eprintln!("{}", format_diagnostic(diagnostic));
        }
    }
    let site = Site::new(&snapshot)?;
    let rendered = render_site(&site)?;
    rendered.publish(output)?;
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
