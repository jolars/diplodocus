//! Native Julia checkout validation and completion run during extraction.

use super::*;
use crate::extractors::julia;
use std::process::Stdio;
use std::time::Duration;

pub(crate) async fn assemble_for_extraction(
    path: &Path,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
    reporting: &Reporting,
    journal: &mut super::r_native::GeneratedWrites,
) -> Result<WorkspaceSources, AssemblyError> {
    let loading = reporting.stage("Loading workspace", None);
    let configuration = crate::configuration::load_configuration(path)?;
    reject_errors(validate_configuration_with_source(
        &configuration,
        "diplodocus.yaml".try_into().unwrap(),
    ))?;
    let paths = resolve_workspace_paths(path, &configuration)?;
    loading.finish();
    let mut progress = reporting.stage(
        "Extracting Julia APIs",
        Some(target_count(&configuration, "julia")),
    );
    let mut results = BTreeMap::new();
    let mut cache_observations = Vec::new();
    for (package_index, package) in paths.packages.iter().enumerate() {
        let repository = &paths.repositories[package.repository_index];
        let declaration = configuration
            .packages
            .iter()
            .find(|p| p.id == package.id)
            .unwrap();
        let extraction_cache =
            crate::incremental::package_context(&configuration, &paths, package_index);
        let environment = if extraction_cache.is_some()
            && declaration
                .targets
                .iter()
                .any(|target| target.extractor == "julia")
        {
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
            let timing = reporting.phase(Phase::Extraction, Some(&package.id), Some(&target.id));
            let mut result = super::packages::cached_fragment(
                extraction_cache.as_ref(),
                "static-julia",
                &package.id,
                reporting,
                &(declaration, target),
                |result: &julia::JuliaExtraction| {
                    !result
                        .diagnostics
                        .iter()
                        .any(|diagnostic| diagnostic.severity == Severity::Error)
                        && super::packages::evidence_current(&result.provenance, repository)
                },
                || julia::extract_target(repository, package, selected),
            );
            reject_errors(
                result
                    .diagnostics
                    .iter()
                    .filter(|d| d.code != DiagnosticCode::JuliaUnresolvedDefinition)
                    .cloned()
                    .collect(),
            )?;
            timing.finish();
            let cache = environment.as_ref().and_then(|environment| {
                super::julia_environment::Context::new(
                    extraction_cache.clone(),
                    environment,
                    "julia-runtime",
                    &package.id,
                    &(declaration, target, &result),
                    deadlines,
                )
            });
            if let Some(cache) = &cache {
                cache_observations.extend(cache.manifests().iter().cloned());
                if let Some(candidate) = cache.load::<julia::JuliaExtraction>(reporting)
                    && candidate.target == result.target
                    && super::packages::evidence_current(&candidate.provenance, repository)
                    && !candidate
                        .diagnostics
                        .iter()
                        .any(|diagnostic| diagnostic.severity == Severity::Error)
                {
                    results.insert((package.id.clone(), target.id.clone()), candidate);
                    progress.advance();
                    continue;
                }
            } else if extraction_cache.is_some() {
                reporting.cache(
                    "julia-runtime",
                    &package.id,
                    false,
                    Some("Prepared Julia dependency coverage is uncertain."),
                );
            }
            let reflect = result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::JuliaUnresolvedDefinition);
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
                .args(["--startup-file=no", "--history-file=no"])
                .arg(format!("--project={}", package.path.display()))
                .arg(&helper)
                .arg(&package.path)
                .arg(&selected.path)
                .arg(&output)
                .arg(if reflect { "true" } else { "false" })
                .current_dir(&package.path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(fs::File::create(&errors)?)
                .kill_on_drop(true);
            #[cfg(target_os = "linux")]
            command.process_group(0);
            let timing = reporting.phase(Phase::NativeProcess, Some(&package.id), Some(&target.id));
            let child = command.spawn().map_err(|_| {
                failure(
                    "Julia extraction requires Julia and the prepared checkout dependencies."
                        .into(),
                )
            })?;
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
                            return Err(failure("Julia runtime extraction timed out.".into()));
                        }
                    }
                }
                () = cancellation.as_mut() => {
                    rd_examples::terminate(&mut process, deadlines).await?;
                    return Err(AssemblyError::Cancelled);
                }
            };
            reporting.native_timings(
                &output.with_extension("timings"),
                &package.id,
                Some(&target.id),
            );
            if !status.success() {
                let message = fs::read_to_string(errors)?
                    .replace(repository.path.to_string_lossy().as_ref(), "<repository>")
                    .replace(staging.path().to_string_lossy().as_ref(), "<julia-helper>");
                return Err(failure(format!(
                    "Julia runtime extraction failed: {}",
                    message.trim()
                )));
            }
            timing.finish();
            let observation = toml::from_str(&fs::read_to_string(output)?)
                .map_err(|e| failure(format!("Invalid Julia runtime observations: {e}")))?;
            julia::runtime::reconcile(&mut result, observation).map_err(failure)?;
            reject_errors(result.diagnostics.clone())?;
            let cache_complete = cache
                .as_ref()
                .is_some_and(|cache| cache.accepts_completion(staging.path(), reporting));
            staging.close()?;
            if let Some(cache) = &cache
                && cache_complete
            {
                cache.save(&result);
            }
            results.insert((package.id.clone(), target.id.clone()), result);
            progress.advance();
        }
    }
    progress.finish();
    let rust = super::rust_results(path, true, deadlines, cancellation, reporting, false).await?;
    let doxygen = super::doxygen_results(path, deadlines, cancellation, reporting).await?;
    let r = super::r_results(path, deadlines, cancellation, reporting, journal).await?;
    let python = super::python_native::results(path, deadlines, cancellation, reporting).await?;
    let mut sources =
        super::assemble_with_native(path, results, rust, doxygen, r, python, reporting)?;
    sources.cache_observations.extend(cache_observations);
    Ok(sources)
}
