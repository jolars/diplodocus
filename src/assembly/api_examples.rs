//! Native doctest adapters share the supervised process and publication boundary.

use super::*;
use crate::diagnostics::DiagnosticEntity;
use crate::ir::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::process::Stdio;
use std::time::Duration;

#[derive(Serialize)]
struct Request<'a> {
    package_root: &'a Path,
    repository_root: &'a Path,
    metadata_path: &'a Path,
    import_roots: Vec<PathBuf>,
    tracked_source_paths: Vec<PathBuf>,
    qualified_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    docstring_module: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    doctest_lines: Option<&'a [[usize; 2]]>,
    package_name: &'a str,
    package_version: &'a str,
    source: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    portable_source: Option<&'a str>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Results {
    runtime_version: String,
    package_name: String,
    package_version: String,
    dependencies: BTreeMap<String, String>,
    examples: Vec<ApiExampleOutcome>,
    #[serde(default)]
    environment: Option<PythonEnvironment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PythonEnvironment {
    search_paths: Vec<PathBuf>,
    runtime_paths: Vec<PathBuf>,
    used_paths: Vec<PathBuf>,
    flags: Vec<i64>,
    eligible: bool,
}

impl PythonEnvironment {
    pub(super) fn covers(
        &self,
        completed: &Self,
        manifests: &[&crate::incremental::Manifest],
    ) -> bool {
        if !completed.eligible
            || self.search_paths != completed.search_paths
            || self.flags != completed.flags
        {
            return false;
        }
        let covered: BTreeSet<_> = manifests
            .iter()
            .flat_map(|manifest| manifest.paths().chain(manifest.resolved_paths()))
            .collect();
        completed
            .runtime_paths
            .iter()
            .chain(&completed.used_paths)
            .all(|path| covered.contains(path))
    }

    pub(super) fn observe(&self) -> Option<crate::incremental::Manifest> {
        if !self.eligible {
            return None;
        }
        crate::incremental::Manifest::capture(
            &self.runtime_paths,
            &[
                "PATH",
                "LANG",
                "LC_ALL",
                "TZ",
                "LD_LIBRARY_PATH",
                "LD_PRELOAD",
                "PYTHONPATH",
                "PYTHONHOME",
                "PYTHONOPTIMIZE",
                "PYTHONNOUSERSITE",
                "PYTHONUSERBASE",
                "PYTHONSAFEPATH",
                "PYTHONHASHSEED",
                "PYTHONWARNINGS",
            ]
            .map(str::to_owned),
        )
        .ok()
    }
}

/// Recognized Julia executable syntax; ordinary language fences remain illustrative.
fn recognized_julia(document: &SourcedDocument) -> bool {
    let Some(raw) = &document.raw_source else {
        return false;
    };
    match &document.source_format {
        DocumentFormat::Extracted { name } if name == "julia-markdown" => raw.lines().any(|line| {
            let line = line.trim_start();
            (line.starts_with("```") || line.starts_with("~~~"))
                && ["jldoctest", "@example", "@repl"]
                    .iter()
                    .any(|prefix| line.trim_start_matches(['`', '~']).starts_with(prefix))
        }),
        _ => false,
    }
}

fn failure(package: &str, source: &SourceLocation, message: impl Into<String>) -> AssemblyError {
    AssemblyError::Diagnostics(vec![
        Diagnostic::new(
            DiagnosticCode::ExecutionCellFailed,
            Severity::Error,
            message,
        )
        .with_source(DiagnosticSource::Repository {
            repository: source.repository.clone(),
            path: source.path.clone(),
        })
        .with_entity(DiagnosticEntity::Package { id: package.into() }),
    ])
}

impl WorkspaceSources {
    /// Load Python checkouts and verify Python and Julia doctests before publication.
    ///
    /// Python checkouts are verified on every attempt before completed examples
    /// can be restored. Fresh execution uses a separate process per documentation
    /// unit. Missing prerequisites, mismatches, timeouts, and cancellation fail
    /// extraction.
    pub async fn execute_api_examples(
        &mut self,
        deadlines: ExecutionDeadlines,
        cancellation: &mut ExecutionCancellation<'_>,
    ) -> Result<(), AssemblyError> {
        for (package_index, package) in self.paths.packages.iter().enumerate() {
            let record = &self.workspace.packages[&package.id];
            if !matches!(record.ecosystem.as_str(), "python" | "julia") {
                continue;
            }
            let language = record.ecosystem.clone();
            let version = record.version.clone().unwrap_or_default();
            let julia_package = match &record.metadata {
                Some(PackageMetadata::Julia(metadata)) => Some(metadata.name.clone()),
                _ => None,
            };
            let package_cache = (language == "python")
                .then(|| {
                    crate::incremental::package_context(
                        &self.configuration,
                        &self.paths,
                        package_index,
                    )
                })
                .flatten();
            let python_environment = self.python_environments.get(&package.id).cloned();
            // Hash the shared discovery environment once. Each document keeps
            // only dependencies loaded in addition to that verified baseline.
            let python_environment_key = python_environment.as_ref().map(crate::incremental::key);
            if package_cache.is_some() && python_environment.is_none() {
                self.reporting.cache(
                    "python-api-examples",
                    &package.id,
                    false,
                    Some("Prepared Python dependency coverage is uncertain."),
                );
            }
            let julia_base = (language == "julia")
                .then(|| {
                    crate::incremental::package_context(
                        &self.configuration,
                        &self.paths,
                        package_index,
                    )
                })
                .flatten();
            let julia_environment = if julia_base.is_some() {
                let validation =
                    self.reporting
                        .phase(Phase::CacheValidation, Some(&package.id), None);
                let environment = super::julia_environment::observe(
                    &self.paths.configuration_directory,
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
            let julia_cache = julia_environment.as_ref().and_then(|environment| {
                super::julia_environment::Context::new(
                    julia_base.clone(),
                    environment,
                    "julia-api-examples",
                    &package.id,
                    record,
                    deadlines,
                )
            });
            let mut julia_complete = true;
            if let Some(cache) = &julia_cache {
                self.cache_observations
                    .extend(cache.manifests().iter().cloned());
                if let Some(candidate) = cache.load::<crate::ir::Package>(&self.reporting)
                    && candidate.ecosystem == "julia"
                    && candidate.version == record.version
                    && candidate.repository == record.repository
                {
                    self.workspace
                        .packages
                        .insert(package.id.clone(), candidate);
                    continue;
                }
            } else if julia_base.is_some() {
                self.reporting.cache(
                    "julia-api-examples",
                    &package.id,
                    false,
                    Some("Prepared Julia dependency coverage is uncertain."),
                );
            }
            let doctests = self.python_doctests.get(&package.id);
            let selected: Vec<_> = record
                .items
                .iter()
                .filter_map(|(id, item)| {
                    let doctest = doctests.and_then(|plans| plans.get(id));
                    let document = item
                        .documentation
                        .as_ref()
                        .filter(|document| doctest.is_some() || recognized_julia(document))?;
                    Some((
                        id.clone(),
                        item.qualified_name.clone(),
                        document.clone(),
                        doctest.cloned(),
                    ))
                })
                .collect();
            let repository = &self.paths.repositories[package.repository_index];
            let label = if language == "python" {
                "Checking Python APIs and examples"
            } else {
                "Executing Julia API examples"
            };
            let mut progress = self.reporting.stage(label, Some(selected.len()));
            for (id, qualified_name, document, doctest) in selected {
                progress.item(&format!("{}:{qualified_name}", package.id));
                self.revalidate()?;
                let location = document.source_location.as_ref().ok_or_else(|| {
                    AssemblyError::Diagnostics(vec![Diagnostic::new(
                        DiagnosticCode::ExecutionCellFailed,
                        Severity::Error,
                        format!("API examples for `{qualified_name}` need a verified documentation source file."),
                    ).with_entity(DiagnosticEntity::Item { package: package.id.clone(), id: id.clone() })])
                })?;
                if document
                    .provenance
                    .iter()
                    .any(|p| matches!(p.activity, ProvenanceActivity::ApiExamples { .. }))
                {
                    return Err(AssemblyError::InputsChanged);
                }
                let raw = document.raw_source.as_deref().unwrap_or_default();
                let execution_source = doctest.as_ref().map_or(raw, |plan| plan.source.as_str());
                let docstring_module = doctest.as_ref().map(|plan| plan.module.as_str());
                let doctest_lines = doctest.as_ref().map(|plan| plan.lines.as_slice());
                let cache_key = package_cache
                    .as_ref()
                    .zip(python_environment_key.as_ref())
                    .map(|((_, build, inputs), environment)| {
                        crate::incremental::key(&(
                            "python-api-examples-v6",
                            build,
                            &package.id,
                            &id,
                            &qualified_name,
                            docstring_module,
                            doctest_lines,
                            fingerprint_bytes(execution_source.as_bytes()),
                            &version,
                            &document,
                            inputs,
                            environment,
                            [
                                deadlines.startup,
                                deadlines.cell,
                                deadlines.shutdown,
                                deadlines.forced_exit,
                            ],
                        ))
                    });
                if let Some(((store, _, inputs), key)) =
                    package_cache.as_ref().zip(cache_key.as_ref())
                {
                    let cached: Option<Provenance> = store.load("python-api-examples", key);
                    if let Some(cached) = cached
                        && matches!(&cached.activity, ProvenanceActivity::ApiExamples {
                            package: cached_package, policy, input, examples, ..
                        } if cached_package == &package.id
                            && policy == "python-doctest-v1"
                            && input.source == *location
                            && input.fingerprint == fingerprint_bytes(raw.as_bytes())
                            && !examples.is_empty())
                    {
                        if !self.cache_observations.contains(inputs) {
                            self.cache_observations.push(inputs.clone());
                        }
                        self.workspace
                            .packages
                            .get_mut(&package.id)
                            .unwrap()
                            .items
                            .get_mut(&id)
                            .unwrap()
                            .documentation
                            .as_mut()
                            .unwrap()
                            .provenance
                            .push(cached);
                        self.revalidate()?;
                        self.reporting
                            .cache("python-api-examples", &package.id, true, None);
                        progress.advance();
                        continue;
                    }
                    self.reporting.cache(
                        "python-api-examples",
                        &package.id,
                        false,
                        Some("Example, package, or Python environment inputs changed."),
                    );
                }
                let staging = tempfile::tempdir_in(std::path::absolute(std::env::temp_dir())?)?;
                let request = staging.path().join("request.toml");
                let output = staging.path().join("results");
                let errors = staging.path().join("errors");
                let helper = staging.path().join(if language == "python" {
                    "examples.py"
                } else {
                    "examples.jl"
                });
                let helper_source = if language == "python" {
                    include_str!("api_examples.py").to_owned()
                } else {
                    super::julia_environment::completed_source(
                        include_str!("api_examples.jl"),
                        staging.path(),
                    )?
                };
                fs::write(&helper, helper_source)?;
                if language == "python" {
                    fs::write(
                        staging.path().join("python_environment.py"),
                        include_str!("python_environment.py"),
                    )?;
                }
                let import_roots = if language == "python" {
                    crate::extractors::python::import_roots(package)
                } else {
                    vec![]
                };
                let package_name = julia_package
                    .as_deref()
                    .unwrap_or_else(|| qualified_name.split('.').next().unwrap());
                fs::write(
                    &request,
                    toml::to_string(&Request {
                        package_root: &package.path,
                        repository_root: &repository.path,
                        metadata_path: &package.metadata_path,
                        import_roots,
                        tracked_source_paths: package_cache
                            .as_ref()
                            .map(|(_, _, inputs)| inputs.paths().cloned().collect())
                            .unwrap_or_default(),
                        qualified_name: &qualified_name,
                        docstring_module,
                        doctest_lines,
                        package_name,
                        package_version: &version,
                        source: execution_source,
                        portable_source: (execution_source != raw).then_some(raw),
                    })
                    .map_err(|_| AssemblyError::InputsChanged)?,
                )?;
                let mut command = if language == "python" {
                    let python = self
                        .python_interpreter
                        .as_ref()
                        .ok_or(AssemblyError::InputsChanged)?;
                    super::python_native::command(python, staging.path())
                } else {
                    tokio::process::Command::new("julia")
                };
                if language == "julia" {
                    command
                        .args(["--startup-file=no", "--history-file=no"])
                        .arg(format!("--project={}", package.path.display()));
                }
                command
                    .arg(&helper)
                    .arg(&request)
                    .arg(&output)
                    .current_dir(&package.path)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(fs::File::create(&errors)?)
                    .kill_on_drop(true);
                #[cfg(target_os = "linux")]
                command.process_group(0);
                let timing = self
                    .reporting
                    .phase(Phase::NativeProcess, Some(&package.id), None);
                let child = command.spawn().map_err(|_| failure(&package.id, location, format!("{language} API examples require the interpreter and prepared checkout dependencies.")))?;
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
                                return Err(failure(&package.id, location, format!("{language} API example execution timed out.")));
                            }
                        }
                    }
                    () = cancellation.as_mut() => {
                        rd_examples::terminate(&mut process, deadlines).await?;
                        return Err(AssemblyError::Cancelled);
                    }
                };
                drop(process);
                self.reporting
                    .native_timings(&output.with_extension("timings"), &package.id, None);
                if !status.success() {
                    let mut message = fs::read_to_string(&errors).unwrap_or_default();
                    if language == "julia" {
                        message = super::julia_runtime::portable_error(&message);
                    }
                    let message = message
                        .replace(repository.path.to_string_lossy().as_ref(), "<repository>")
                        .replace(staging.path().to_string_lossy().as_ref(), "<examples>");
                    return Err(failure(
                        &package.id,
                        location,
                        format!(
                            "{language} API example execution failed: {}",
                            message.trim()
                        ),
                    ));
                }
                timing.finish();
                let result = fs::read_to_string(&output)?;
                let results: Results = if language == "python" {
                    serde_json::from_str(&result).map_err(|_| ())
                } else {
                    toml::from_str(&result).map_err(|_| ())
                }
                .map_err(|()| {
                    failure(&package.id, location, "Invalid native API example results.")
                })?;
                if results.examples.is_empty()
                    || results.package_name != package_name
                    || results.package_version != version
                {
                    return Err(failure(
                        &package.id,
                        location,
                        "API example results do not match the supplied package or examples.",
                    ));
                }
                let mut examples = results.examples;
                let portable = |text: &str| {
                    text.replace(staging.path().to_string_lossy().as_ref(), "<examples>")
                        .replace(repository.path.to_string_lossy().as_ref(), "<repository>")
                };
                for example in &mut examples {
                    example.actual = portable(&example.actual);
                    if language == "python" {
                        example.source = portable(&example.source);
                        example.expected = portable(&example.expected);
                    }
                }
                if let Some(cache) = &julia_cache {
                    julia_complete &= cache.accepts_completion(staging.path(), &self.reporting);
                }
                staging.close()?;
                self.revalidate()?;
                let activity = ProvenanceActivity::ApiExamples {
                    package: package.id.clone(),
                    policy: format!("{language}-doctest-v1"),
                    language: language.clone(),
                    runtime_version: results.runtime_version,
                    package_name: results.package_name,
                    package_version: results.package_version,
                    dependencies: results.dependencies,
                    input: InputFingerprint {
                        source: location.clone(),
                        fingerprint: fingerprint_bytes(raw.as_bytes()),
                    },
                    examples,
                };
                let covered = results.environment.as_ref().is_some_and(|environment| {
                    python_environment
                        .as_ref()
                        .zip(package_cache.as_ref())
                        .is_some_and(|((base, runtime), (_, _, inputs))| {
                            base.covers(environment, &[runtime, inputs])
                        })
                });
                if !covered
                    && let Some(environment) = results
                        .environment
                        .and_then(|environment| environment.observe())
                {
                    self.cache_observations.push(environment);
                }
                let provenance = Provenance {
                    activity,
                    source: Some(DiagnosticSource::Repository {
                        repository: location.repository.clone(),
                        path: location.path.clone(),
                    }),
                    span: location.span,
                    tools: BTreeMap::from([(
                        "diplodocus".into(),
                        env!("CARGO_PKG_VERSION").into(),
                    )]),
                };
                self.workspace
                    .packages
                    .get_mut(&package.id)
                    .unwrap()
                    .items
                    .get_mut(&id)
                    .unwrap()
                    .documentation
                    .as_mut()
                    .unwrap()
                    .provenance
                    .push(provenance.clone());
                self.revalidate()?;
                if let Some(((store, _, inputs), key)) =
                    package_cache.as_ref().zip(cache_key.as_ref())
                    && covered
                    && inputs.verify()
                {
                    let _ = store.save("python-api-examples", key, &provenance);
                }
                progress.advance();
            }
            progress.finish();
            if let Some(cache) = &julia_cache
                && julia_complete
            {
                cache.save(&self.workspace.packages[&package.id]);
            }
        }
        Ok(())
    }
}
