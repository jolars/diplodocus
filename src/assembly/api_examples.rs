//! Native doctest adapters share the supervised process and publication boundary.

use super::*;
use crate::diagnostics::DiagnosticEntity;
use crate::ir::*;
use serde::{Deserialize, Serialize};
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
    package_name: &'a str,
    package_version: &'a str,
    source: &'a str,
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
struct PythonEnvironment {
    search_paths: Vec<PathBuf>,
    runtime_paths: Vec<PathBuf>,
    flags: Vec<i64>,
    eligible: bool,
}

impl PythonEnvironment {
    fn observe(&self) -> Option<crate::incremental::Manifest> {
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

/// Recognized executable syntax; ordinary language fences remain illustrative.
pub(crate) fn recognized(document: &SourcedDocument) -> bool {
    let Some(raw) = &document.raw_source else {
        return false;
    };
    match &document.source_format {
        DocumentFormat::Extracted { name } if name == "numpy-docstring" => raw
            .lines()
            .filter_map(|line| line.trim_start().strip_prefix(">>>"))
            .any(|source| {
                !source.trim().is_empty()
                    && (!source.trim_start().starts_with('#') || source.contains("doctest:"))
            }),
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
            let mut package_cache = (language == "python")
                .then(|| {
                    crate::incremental::package_context(
                        &self.configuration,
                        &self.paths,
                        package_index,
                    )
                })
                .flatten();
            let mut python_environment: Option<(PythonEnvironment, crate::incremental::Manifest)> =
                None;
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
            let mut selected: Vec<_> = record
                .items
                .iter()
                .filter_map(|(id, item)| {
                    item.documentation
                        .as_ref()
                        .filter(|d| recognized(d))
                        .map(|d| {
                            (
                                Some(id.clone()),
                                item.qualified_name.clone(),
                                Some(d.clone()),
                                d.source_location.clone(),
                            )
                        })
                })
                .collect();
            let repository = &self.paths.repositories[package.repository_index];
            if language == "python" {
                // Module identities exist even when no API item has documentation.
                let modules: BTreeMap<_, _> = record
                    .items
                    .values()
                    .filter(|item| item.kind == ItemKind::Module)
                    .filter_map(|item| {
                        item.source_location.as_ref().map(|location| {
                            (
                                item.qualified_name.split('.').next().unwrap().to_owned(),
                                location.clone(),
                            )
                        })
                    })
                    .collect();
                if modules.is_empty() {
                    return Err(failure(
                        &package.id,
                        &repository.source_location(&package.metadata_path, None)?,
                        "Python checkout validation requires an importable module in the declared targets.",
                    ));
                }
                for (name, location) in modules {
                    selected.insert(0, (None, name, None, Some(location)));
                }
            }
            let label = if language == "python" {
                "Checking Python APIs and examples"
            } else {
                "Executing Julia API examples"
            };
            let mut progress = self.reporting.stage(label, Some(selected.len()));
            for (id, qualified_name, document, location) in selected {
                progress.item(&format!("{}:{qualified_name}", package.id));
                self.revalidate()?;
                // A module probe must establish its own coverage, while earlier guards remain.
                if id.is_none() {
                    python_environment = None;
                }
                let operation = if id.is_none() {
                    "checkout validation"
                } else {
                    "API example execution"
                };
                let location = location.as_ref().ok_or(AssemblyError::InputsChanged)?;
                if document.as_ref().is_some_and(|document| {
                    document
                        .provenance
                        .iter()
                        .any(|p| matches!(p.activity, ProvenanceActivity::ApiExamples { .. }))
                }) {
                    return Err(AssemblyError::InputsChanged);
                }
                let raw = document
                    .as_ref()
                    .and_then(|d| d.raw_source.as_deref())
                    .unwrap_or_default();
                let cache_key = package_cache
                    .as_ref()
                    .zip(python_environment.as_ref())
                    .and_then(|((_, build, inputs), (environment, runtime))| {
                        id.as_ref().map(|id| {
                            crate::incremental::key(&(
                                "python-api-examples-v1",
                                build,
                                &package.id,
                                id,
                                &qualified_name,
                                &version,
                                &document,
                                inputs,
                                environment,
                                runtime,
                                [
                                    deadlines.startup,
                                    deadlines.cell,
                                    deadlines.shutdown,
                                    deadlines.forced_exit,
                                ],
                            ))
                        })
                    });
                if let Some(((store, _, inputs), key)) =
                    package_cache.as_ref().zip(cache_key.as_ref())
                {
                    let cached: Option<Provenance> = store.load("python-api-examples", key);
                    if let Some(provenance) = cached
                        && matches!(&provenance.activity, ProvenanceActivity::ApiExamples {
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
                            .get_mut(id.as_ref().unwrap())
                            .unwrap()
                            .documentation
                            .as_mut()
                            .unwrap()
                            .provenance
                            .push(provenance);
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
                let staging = tempfile::tempdir()?;
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
                let import_roots = package
                    .targets
                    .iter()
                    .filter_map(|target| target.path.parent().map(Path::to_owned))
                    .chain(std::iter::once(package.path.clone()))
                    .collect();
                let package_name = qualified_name.split('.').next().unwrap();
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
                        package_name,
                        package_version: &version,
                        source: raw,
                    })
                    .map_err(|_| AssemblyError::InputsChanged)?,
                )?;
                let mut command = tokio::process::Command::new(if language == "python" {
                    "python"
                } else {
                    "julia"
                });
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
                                return Err(failure(&package.id, location, format!("{language} {operation} timed out.")));
                            }
                        }
                    }
                    () = cancellation.as_mut() => {
                        rd_examples::terminate(&mut process, deadlines).await?;
                        return Err(AssemblyError::Cancelled);
                    }
                };
                self.reporting
                    .native_timings(&output.with_extension("timings"), &package.id, None);
                if !status.success() {
                    let message = fs::read_to_string(&errors)
                        .unwrap_or_default()
                        .replace(repository.path.to_string_lossy().as_ref(), "<repository>")
                        .replace(staging.path().to_string_lossy().as_ref(), "<examples>");
                    return Err(failure(
                        &package.id,
                        location,
                        format!("{language} {operation} failed: {}", message.trim()),
                    ));
                }
                timing.finish();
                let result = fs::read_to_string(&output)?;
                let mut results: Results = if language == "python" {
                    serde_json::from_str(&result).map_err(|_| ())
                } else {
                    toml::from_str(&result).map_err(|_| ())
                }
                .map_err(|()| {
                    failure(&package.id, location, "Invalid native API example results.")
                })?;
                if (id.is_some() && results.examples.is_empty())
                    || results.package_name != package_name
                    || results.package_version != version
                {
                    return Err(failure(
                        &package.id,
                        location,
                        "API example results do not match the supplied package or examples.",
                    ));
                }
                for example in &mut results.examples {
                    example.actual = example
                        .actual
                        .replace(repository.path.to_string_lossy().as_ref(), "<repository>")
                        .replace(staging.path().to_string_lossy().as_ref(), "<examples>");
                }
                self.revalidate()?;
                let Some(id) = id else {
                    if package_cache.is_some()
                        && let Some(environment) = results.environment.take()
                    {
                        package_cache = crate::incremental::package_context(
                            &self.configuration,
                            &self.paths,
                            package_index,
                        );
                        if let Some(runtime) = environment.observe() {
                            self.cache_observations.push(runtime.clone());
                            python_environment = Some((environment, runtime));
                        } else {
                            self.reporting.cache(
                                "python-api-examples",
                                &package.id,
                                false,
                                Some("Python runtime dependency coverage is uncertain."),
                            );
                        }
                    }
                    staging.close()?;
                    progress.advance();
                    continue;
                };
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
                    examples: results.examples,
                };
                let result_environment = results.environment.take();
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
                if let Some(cache) = &julia_cache {
                    julia_complete &= cache.accepts_completion(staging.path(), &self.reporting);
                }
                staging.close()?;
                self.revalidate()?;
                if let Some(((store, _, inputs), key)) =
                    package_cache.as_ref().zip(cache_key.as_ref())
                    && let Some((environment, _)) = python_environment.as_ref()
                    && result_environment.as_ref() == Some(environment)
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
