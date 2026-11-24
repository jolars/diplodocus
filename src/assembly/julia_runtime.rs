//! Run native Julia discovery with checkout and environment verification.

use std::collections::BTreeSet;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::*;
use crate::extractors::julia;
use crate::incremental::Manifest;

pub(super) struct JuliaNativeExtraction {
    pub(super) extraction: julia::JuliaExtraction,
    pub(super) completed: crate::ir::Provenance,
    pub(super) observations: Vec<Manifest>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cached {
    extraction: julia::JuliaExtraction,
    completed: crate::ir::Provenance,
    inputs: Manifest,
}

const STAGE: &str = "julia-native";

pub(super) fn portable_error(text: &str) -> String {
    text.lines()
        .map(|line| {
            // Julia's ProcessFailedException displays Cmd's whole environment.
            line.split_once("Process(setenv(").map_or_else(
                || line.to_owned(),
                |(prefix, rest)| {
                    let status = rest
                        .rsplit_once("ProcessExited(")
                        .and_then(|(_, suffix)| suffix.split_once(')'))
                        .map(|(code, _)| code)
                        .filter(|code| code.parse::<i32>().is_ok());
                    match status {
                        Some(code) => {
                            format!("{prefix}Julia subprocess exited with status {code}.")
                        }
                        None => format!("{prefix}Julia subprocess failed."),
                    }
                },
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) async fn results(
    path: &Path,
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
    reporting: &Reporting,
) -> Result<BTreeMap<(String, String), JuliaNativeExtraction>, AssemblyError> {
    let config_input = Manifest::capture(&[std::path::absolute(path)?], &[])?;
    let mut progress = reporting.stage(
        "Extracting Julia APIs",
        Some(target_count(configuration, "julia")),
    );
    let mut results = BTreeMap::new();
    for (package_index, (declaration, package)) in configuration
        .packages
        .iter()
        .zip(&paths.packages)
        .enumerate()
    {
        if !declaration
            .targets
            .iter()
            .any(|target| target.extractor == "julia")
        {
            continue;
        }
        let repository = &paths.repositories[package.repository_index];
        let mut project_paths: Vec<_> = [
            "Project.toml",
            "JuliaProject.toml",
            "Manifest.toml",
            "JuliaManifest.toml",
            "LocalPreferences.toml",
        ]
        .map(|filename| package.path.join(filename))
        .into();
        project_paths.extend((10..=13).flat_map(|minor| {
            [
                format!("Manifest-v1.{minor}.toml"),
                format!("JuliaManifest-v1.{minor}.toml"),
            ]
            .map(|filename| package.path.join(filename))
        }));
        project_paths.push(package.metadata_path.clone());
        let project_inputs = Manifest::capture(&project_paths, &[])?;
        let extraction_cache =
            crate::incremental::package_context(configuration, paths, package_index);
        let environment = if extraction_cache.is_some() {
            let validation = reporting.phase(Phase::CacheValidation, Some(&package.id), None);
            let environment = super::julia_environment::observe(
                &paths.configuration_directory,
                &package.path,
                &package.path,
                &package.path,
                deadlines,
                cancellation,
            )
            .await?;
            validation.finish();
            environment
        } else {
            None
        };
        for (target, selected) in declaration.targets.iter().zip(&package.targets) {
            if target.extractor != "julia" {
                continue;
            }
            progress.item(&format!("{}:{}", package.id, target.id));
            let cache = environment.as_ref().and_then(|environment| {
                super::julia_environment::Context::new(
                    extraction_cache.clone(),
                    environment,
                    STAGE,
                    &package.id,
                    &(declaration, target),
                    deadlines,
                )
            });
            let mut observations = cache
                .as_ref()
                .map_or_else(Vec::new, |cache| cache.manifests().to_vec());
            let cached = cache
                .as_ref()
                .and_then(|cache| cache.load::<Cached>(reporting))
                .filter(|candidate| {
                    candidate.extraction.target.package == package.id
                        && candidate.extraction.target.target == target.id
                        && candidate.inputs.verify()
                        && super::packages::evidence_current(
                            &candidate.extraction.provenance,
                            repository,
                        )
                });
            let cached = if let Some(cached) = cached {
                cached
            } else {
                if cache.is_none() && extraction_cache.is_some() {
                    reporting.cache(
                        STAGE,
                        &package.id,
                        false,
                        Some("Prepared Julia dependency coverage is uncertain."),
                    );
                }
                let failure = |message: String| {
                    AssemblyError::Diagnostics(vec![
                        Diagnostic::new(
                            DiagnosticCode::JuliaConflictingSurface,
                            Severity::Error,
                            message,
                        )
                        .with_entity(
                            crate::diagnostics::DiagnosticEntity::Target {
                                package: package.id.clone(),
                                id: target.id.clone(),
                            },
                        ),
                    ])
                };
                let before = Manifest::capture(std::slice::from_ref(&selected.path), &[])?;
                let staging = tempfile::tempdir()?;
                let output = staging.path().join("observations.toml");
                let errors = staging.path().join("errors");
                let helper = staging.path().join("runtime.jl");
                fs::write(
                    &helper,
                    super::julia_environment::completed_source(
                        include_str!("../extractors/julia/runtime.jl"),
                        staging.path(),
                    )?,
                )?;
                let mut command = tokio::process::Command::new("julia");
                command
                    .args(["--startup-file=no", "--history-file=no", "--color=no"])
                    .arg(format!("--project={}", package.path.display()))
                    .arg(&helper)
                    .arg(&package.path)
                    .arg(&selected.path)
                    .arg(&output)
                    .current_dir(&package.path)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(fs::File::create(&errors)?)
                    .kill_on_drop(true);
                #[cfg(target_os = "linux")]
                command.process_group(0);
                let timing =
                    reporting.phase(Phase::NativeProcess, Some(&package.id), Some(&target.id));
                let child = command.spawn().map_err(|error| failure(format!(
                    "Julia extraction requires Julia and the prepared checkout dependencies: {error}"
                )))?;
                #[cfg(target_os = "linux")]
                let group = child
                    .id()
                    .and_then(|pid| rustix::process::Pid::from_raw(pid as i32));
                let mut process = rd_examples::Process {
                    child,
                    #[cfg(target_os = "linux")]
                    group,
                };
                let status = tokio::select! {
                    result = tokio::time::timeout(Duration::from_millis(deadlines.cell.max(1)), process.child.wait()) => {
                        match result {
                            Ok(status) => status?,
                            Err(_) => {
                                rd_examples::terminate(&mut process, deadlines).await?;
                                return Err(failure("Julia native extraction timed out.".into()));
                            }
                        }
                    }
                    () = cancellation.as_mut() => {
                        rd_examples::terminate(&mut process, deadlines).await?;
                        return Err(AssemblyError::Cancelled);
                    }
                };
                drop(process);
                reporting.native_timings(
                    &output.with_extension("timings"),
                    &package.id,
                    Some(&target.id),
                );
                let portable = |text: &str| {
                    text.replace(repository.path.to_string_lossy().as_ref(), "<repository>")
                        .replace(staging.path().to_string_lossy().as_ref(), "<julia-helper>")
                };
                if !status.success() {
                    return Err(failure(format!(
                        "Julia runtime extraction failed: {}",
                        portable(&portable_error(&fs::read_to_string(errors)?)).trim()
                    )));
                }
                timing.finish();
                let observation: julia::runtime::Observation =
                    toml::from_str(&fs::read_to_string(output)?).map_err(|error| {
                        failure(format!(
                            "Invalid Julia runtime observations: {}",
                            portable(&error.to_string())
                        ))
                    })?;
                let observed_paths = observation.input_paths(package);
                let inputs = Manifest::capture(&observed_paths, &[])?;
                if !before.verify()
                    || !project_inputs.verify()
                    || !observation.verify(package).unwrap_or(false)
                {
                    return Err(AssemblyError::InputsChanged);
                }
                let cache_complete = observation.complete
                    && cache.as_ref().is_some_and(|cache| {
                        let covered: BTreeSet<_> =
                            cache.manifests().iter().flat_map(Manifest::paths).collect();
                        observed_paths.iter().all(|path| covered.contains(path))
                            && cache.accepts_completion(staging.path(), reporting)
                    });
                let timing =
                    reporting.phase(Phase::Extraction, Some(&package.id), Some(&target.id));
                let (mut extraction, completed) =
                    julia::runtime::reconcile(repository, package, selected, observation)
                        .map_err(|message| failure(portable(&message)))?;
                for diagnostic in &mut extraction.diagnostics {
                    diagnostic.message = portable(&portable_error(&diagnostic.message));
                    diagnostic.related_entity.get_or_insert_with(|| {
                        crate::diagnostics::DiagnosticEntity::Target {
                            package: package.id.clone(),
                            id: target.id.clone(),
                        }
                    });
                }
                reject_errors(extraction.diagnostics.clone())?;
                if !inputs.verify()
                    || !before.verify()
                    || !project_inputs.verify()
                    || !super::packages::evidence_current(&extraction.provenance, repository)
                {
                    return Err(AssemblyError::InputsChanged);
                }
                timing.finish();
                staging.close()?;
                let value = Cached {
                    extraction,
                    completed,
                    inputs,
                };
                if let Some(cache) = &cache
                    && cache_complete
                {
                    cache.save(&value);
                }
                value
            };
            observations.extend([cached.inputs, project_inputs.clone(), config_input.clone()]);
            results.insert(
                (package.id.clone(), target.id.clone()),
                JuliaNativeExtraction {
                    extraction: cached.extraction,
                    completed: cached.completed,
                    observations,
                },
            );
            progress.advance();
        }
    }
    progress.finish();
    Ok(results)
}
