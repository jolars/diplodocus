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
    metadata_path: &'a Path,
    import_roots: Vec<PathBuf>,
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
    /// Verify recognized Python and Julia doctests before snapshot publication.
    ///
    /// Each documentation unit gets a fresh native process and verified checkout.
    /// Missing prerequisites, mismatches, timeouts, and cancellation fail extraction.
    pub async fn execute_api_examples(
        &mut self,
        deadlines: ExecutionDeadlines,
        cancellation: &mut ExecutionCancellation<'_>,
    ) -> Result<(), AssemblyError> {
        for package in &self.paths.packages {
            let record = &self.workspace.packages[&package.id];
            if !matches!(record.ecosystem.as_str(), "python" | "julia") {
                continue;
            }
            let language = record.ecosystem.clone();
            let version = record.version.clone().unwrap_or_default();
            let selected: Vec<_> = record
                .items
                .iter()
                .filter_map(|(id, item)| {
                    item.documentation
                        .as_ref()
                        .filter(|d| recognized(d))
                        .map(|d| (id.clone(), item.qualified_name.clone(), d.clone()))
                })
                .collect();
            let repository = &self.paths.repositories[package.repository_index];
            for (id, qualified_name, document) in selected {
                self.revalidate()?;
                let location = document
                    .source_location
                    .as_ref()
                    .ok_or(AssemblyError::InputsChanged)?;
                if document
                    .provenance
                    .iter()
                    .any(|p| matches!(p.activity, ProvenanceActivity::ApiExamples { .. }))
                {
                    return Err(AssemblyError::InputsChanged);
                }
                let raw = document.raw_source.as_deref().unwrap();
                let staging = tempfile::tempdir()?;
                let request = staging.path().join("request.toml");
                let output = staging.path().join("results");
                let errors = staging.path().join("errors");
                let helper = staging.path().join(if language == "python" {
                    "examples.py"
                } else {
                    "examples.jl"
                });
                fs::write(
                    &helper,
                    if language == "python" {
                        include_str!("api_examples.py")
                    } else {
                        include_str!("api_examples.jl")
                    },
                )?;
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
                        metadata_path: &package.metadata_path,
                        import_roots,
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
                if !status.success() {
                    let message = fs::read_to_string(&errors)
                        .unwrap_or_default()
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
                let result = fs::read_to_string(&output)?;
                let mut results: Results = if language == "python" {
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
                for example in &mut results.examples {
                    example.actual = example
                        .actual
                        .replace(repository.path.to_string_lossy().as_ref(), "<repository>")
                        .replace(staging.path().to_string_lossy().as_ref(), "<examples>");
                }
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
                    .push(Provenance {
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
                    });
                staging.close()?;
                self.revalidate()?;
            }
        }
        Ok(())
    }
}
