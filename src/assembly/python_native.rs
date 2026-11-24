//! Supervised native Python discovery, independent of doctest execution.

use super::*;
use crate::extractors::python::{
    self,
    native::{DoctestPlans, Observations},
};
use crate::incremental::Manifest;
use crate::ir::Provenance;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::io::Read;
use std::process::Stdio;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PythonNativeExtraction {
    pub extraction: python::PythonExtraction,
    pub completed: Provenance,
    pub observations: Vec<Manifest>,
    pub environment: Option<(api_examples::PythonEnvironment, Manifest)>,
    pub interpreter: PathBuf,
    pub doctests: DoctestPlans,
    pub support_items: BTreeSet<String>,
}

fn select_interpreter(working_directory: &Path, path: &OsStr) -> Option<PathBuf> {
    let executable = format!("python{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(path)
        .map(|root| working_directory.join(root).join(&executable))
        .find(|path| {
            let Ok(metadata) = path.metadata() else {
                return false;
            };
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            metadata.is_file()
        })
}

pub(super) fn command(interpreter: &Path, staging: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(interpreter);
    let mut prefix = std::ffi::OsString::from("pycache_prefix=");
    prefix.push(staging.join("bytecode"));
    // Bypass existing bytecode even for modules imported during Python startup.
    command.args(["-B", "-X"]).arg(prefix);
    command
}

struct Staging(tempfile::TempDir);

impl Staging {
    fn new(request: &[u8]) -> std::io::Result<Self> {
        let directory = tempfile::tempdir_in(std::path::absolute(std::env::temp_dir())?)?;
        fs::write(directory.path().join("request.json"), request)?;
        fs::write(
            directory.path().join("export.py"),
            include_str!("python_native.py"),
        )?;
        fs::write(
            directory.path().join("python_environment.py"),
            include_str!("python_environment.py"),
        )?;
        Ok(Self(directory))
    }

    fn path(&self) -> &Path {
        self.0.path()
    }

    fn output(&self) -> PathBuf {
        self.path().join("observations.json")
    }

    fn launch(
        &self,
        interpreter: &Path,
        working_directory: &Path,
    ) -> std::io::Result<tokio::process::Command> {
        let mut command = command(interpreter, self.path());
        command
            .arg(self.path().join("export.py"))
            .arg(self.path().join("request.json"))
            .arg(self.output())
            .current_dir(working_directory)
            .stderr(fs::File::create(self.path().join("errors"))?);
        Ok(command)
    }

    fn close(self) -> std::io::Result<()> {
        self.0.close()
    }
}

pub(super) async fn results(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
    reporting: &Reporting,
) -> Result<BTreeMap<(String, String), PythonNativeExtraction>, AssemblyError> {
    let mut results = BTreeMap::new();
    let mut selected_python = None;
    let mut progress = reporting.stage(
        "Extracting Python APIs",
        Some(target_count(configuration, "python")),
    );
    for (index, package) in paths.packages.iter().enumerate() {
        let declaration = &configuration.packages[index];
        let repository = &paths.repositories[package.repository_index];
        for (target, selected) in declaration.targets.iter().zip(&package.targets) {
            if target.extractor != "python" {
                continue;
            }
            progress.item(&format!("{}:{}", package.id, target.id));
            let failure = |message: String| {
                AssemblyError::Diagnostics(vec![
                    Diagnostic::new(
                        DiagnosticCode::PythonUnsupportedSurface,
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
            let context = crate::incremental::package_context(configuration, paths, index);
            if selected_python.is_none() {
                // Keep the absolute logical executable. Resolving its symlink
                // before launch would bypass a virtual environment's entry point.
                let working_directory = std::env::current_dir()?;
                let interpreter = std::env::var_os("PATH")
                    .and_then(|path| select_interpreter(&working_directory, &path))
                    .ok_or_else(|| {
                        failure("Native Python discovery requires `python` on PATH.".into())
                    })?;
                let selection = Manifest::capture(
                    std::slice::from_ref(&interpreter),
                    &["PATH", "PYTHONPATH", "PYTHONHOME", "PYTHONOPTIMIZE"].map(str::to_owned),
                )
                .map_err(|error| {
                    failure(format!(
                        "Cannot observe the Python interpreter selection: {error}"
                    ))
                })?;
                selected_python = Some((interpreter, selection));
            }
            let (interpreter, selection) = selected_python.as_ref().unwrap();
            let cache_key = context.as_ref().map(|(_, build, inputs)| {
                crate::incremental::key(&(
                    "python-sphinx-v6",
                    build,
                    declaration,
                    target,
                    inputs,
                    &selection,
                    [
                        deadlines.startup,
                        deadlines.cell,
                        deadlines.shutdown,
                        deadlines.forced_exit,
                    ],
                ))
            });
            if let Some(((store, _, inputs), key)) = context.as_ref().zip(cache_key.as_ref()) {
                let cached: Option<PythonNativeExtraction> = store.load("python-native", key);
                if let Some(cached) = cached
                    && cached.extraction.target.package == package.id
                    && cached.extraction.target.target == target.id
                    && cached.interpreter == *interpreter
                    && !cached
                        .extraction
                        .diagnostics
                        .iter()
                        .any(|d| d.severity == Severity::Error)
                    && packages::evidence_current(&cached.extraction.provenance, repository)
                    && cached.environment.is_some()
                    && !cached.observations.is_empty()
                    && crate::incremental::verification_scope(|| {
                        inputs.verify()
                            && selection.verify()
                            && cached.observations.iter().all(Manifest::verify)
                    })
                {
                    reporting.cache("python-native", &package.id, true, None);
                    results.insert((package.id.clone(), target.id.clone()), cached);
                    progress.advance();
                    continue;
                }
                let _ = store.discard("python-native", key);
            }
            reporting.cache(
                "python-native",
                &package.id,
                false,
                Some("Package, interpreter, Sphinx, or prepared dependency inputs changed."),
            );
            let parsed = python::parse_target(repository, package, selected);
            reject_errors(
                parsed
                    .diagnostics
                    .iter()
                    .filter(|d| !python::native::supplemental_diagnostic(&parsed, d))
                    .cloned()
                    .collect(),
            )?;
            let metadata = parsed.metadata.as_ref().ok_or_else(|| {
                failure("Native Python discovery requires valid package metadata.".into())
            })?;
            if parsed.modules.is_empty() {
                return Err(failure("Native Python discovery requires an importable module or a maintained stub in the selected target.".into()));
            }
            let mut modules = BTreeMap::new();
            let mut stub_docs = vec![];
            for module in &parsed.modules {
                let entry = modules.entry(module.name.clone()).or_insert_with(|| serde_json::json!({ "name": module.name, "path": repository.path.join(module.source.path.as_str()), "stub_only": true }));
                if module.kind == python::PythonSourceKind::Source {
                    entry["stub_only"] = false.into();
                }
                if module.kind == python::PythonSourceKind::Stub {
                    if let Some(doc) = &module.docstring {
                        stub_docs.push(serde_json::json!({ "module": module.name, "name": module.name, "kind": "module", "text": doc.text }));
                    }
                    collect_stub_docs(
                        &module.name,
                        &module.name,
                        &module.declarations,
                        &mut stub_docs,
                    );
                }
            }
            let import_roots = python::import_roots(package);
            let mut tracked: Vec<_> = parsed
                .inputs
                .keys()
                .map(|path| repository.path.join(path.as_str()))
                .collect();
            if let Some((_, _, inputs)) = &context {
                tracked.extend(inputs.paths().cloned());
            }
            let request = serde_json::json!({
                "package_root": package.path, "repository_root": repository.path,
                "target_path": selected.path,
                "package_version": metadata.version, "import_roots": import_roots,
                "tracked_source_paths": tracked, "modules": modules.into_values().collect::<Vec<_>>(), "stub_docs": stub_docs,
            });
            let request = serde_json::to_vec(&request).unwrap();
            let before = if context.is_some() {
                let staging = Staging::new(&request)?;
                let mut probe = staging.launch(interpreter, &package.path)?;
                probe.arg("--observe");
                let status = run_helper(probe, deadlines, cancellation, &failure).await?;
                let environment = status
                    .success()
                    .then(|| fs::read(staging.output()).ok())
                    .flatten()
                    .and_then(|bytes| {
                        serde_json::from_slice::<api_examples::PythonEnvironment>(&bytes).ok()
                    });
                staging.close()?;
                environment.and_then(|environment| {
                    environment.observe().map(|inputs| (environment, inputs))
                })
            } else {
                None
            };
            let staging = Staging::new(&request)?;
            let output = staging.output();
            let timing = reporting.phase(Phase::NativeProcess, Some(&package.id), Some(&target.id));
            let status = run_helper(
                staging.launch(interpreter, &package.path)?,
                deadlines,
                cancellation,
                &failure,
            )
            .await?;
            reporting.native_timings(
                &output.with_extension("timings"),
                &package.id,
                Some(&target.id),
            );
            timing.finish();
            if !status.success() {
                let mut message = Vec::new();
                fs::File::open(staging.path().join("errors"))?
                    .take(1_048_576)
                    .read_to_end(&mut message)?;
                let message = String::from_utf8_lossy(&message)
                    .replace(staging.path().to_string_lossy().as_ref(), "<python-helper>")
                    .replace(repository.path.to_string_lossy().as_ref(), "<repository>");
                return Err(failure(format!(
                    "Native Python extraction failed: {}",
                    message.trim()
                )));
            }
            let observed: Observations = serde_json::from_slice(&fs::read(&output)?)
                .map_err(|error| failure(format!("Invalid Python native observations: {error}")))?;
            if observed.inputs_changed
                || parsed.inputs.values().any(|input| {
                    let path = repository.path.join(input.source.path.as_str());
                    observed
                        .inputs
                        .get(&path)
                        .is_some_and(|fingerprint| fingerprint != &input.fingerprint)
                })
            {
                return Err(AssemblyError::InputsChanged);
            }
            // TMPDIR may be inside the selected target or a runtime input.
            staging.close()?;
            // Provenance tracks the resolved files. Keep the paths Python
            // opened as local evidence through publication and cache reuse.
            let aliases = if observed.input_aliases.is_empty() {
                None
            } else {
                let paths: Vec<_> = observed.input_aliases.keys().cloned().collect();
                let inputs =
                    Manifest::capture(&paths, &[]).map_err(|_| AssemblyError::InputsChanged)?;
                if !observed.input_aliases.iter().all(|(path, target)| {
                    observed
                        .inputs
                        .get(target)
                        .is_some_and(|digest| inputs.matches_file(path, target, digest))
                }) {
                    return Err(AssemblyError::InputsChanged);
                }
                Some(inputs)
            };
            let environment: api_examples::PythonEnvironment =
                serde_json::from_value(observed.environment.clone()).map_err(|error| {
                    failure(format!("Invalid Python environment observations: {error}"))
                })?;
            let (extraction, completed, doctests, support_items) =
                python::native::normalize(parsed, &observed, repository, package)
                    .map_err(failure)?;
            reject_errors(extraction.diagnostics.clone())?;
            if !packages::evidence_current(&extraction.provenance, repository)
                || !crate::incremental::verification_scope(|| {
                    selection.verify()
                        && aliases.as_ref().is_none_or(Manifest::verify)
                        && context
                            .as_ref()
                            .is_none_or(|(_, _, inputs)| inputs.verify())
                        && before.as_ref().is_none_or(|(_, inputs)| inputs.verify())
                })
            {
                return Err(AssemblyError::InputsChanged);
            }
            let mut observations = vec![selection.clone()];
            observations.extend(aliases);
            if let Some((_, _, inputs)) = &context {
                observations.push(inputs.clone());
            }
            let completed_environment = before.filter(|(initial, runtime)| {
                context.as_ref().is_some_and(|(_, _, inputs)| {
                    initial.covers(&environment, &[runtime, inputs, selection])
                })
            });
            if let Some((_, inputs)) = &completed_environment {
                observations.push(inputs.clone());
            } else {
                // A fresh result still tracks the runtime files observed by discovery.
                // A post-extraction observation cannot authorize cache reuse.
                observations.push(environment.observe_used()?);
            }
            let result = PythonNativeExtraction {
                extraction,
                completed,
                observations,
                environment: completed_environment,
                interpreter: interpreter.clone(),
                doctests,
                support_items,
            };
            if result.environment.is_some()
                && let Some(((store, _, _), key)) = context.as_ref().zip(cache_key.as_ref())
            {
                let _ = store.save("python-native", key, &result);
            }
            results.insert((package.id.clone(), target.id.clone()), result);
            progress.advance();
        }
    }
    progress.finish();
    Ok(results)
}

async fn run_helper(
    mut command: tokio::process::Command,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
    failure: &impl Fn(String) -> AssemblyError,
) -> Result<std::process::ExitStatus, AssemblyError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .kill_on_drop(true);
    #[cfg(target_os = "linux")]
    command.process_group(0);
    let child = command.spawn().map_err(|_| failure("Native Python extraction requires Python, Sphinx >=9.0.4,<10, and prepared checkout dependencies.".into()))?;
    #[cfg(target_os = "linux")]
    let group = child
        .id()
        .and_then(|pid| rustix::process::Pid::from_raw(pid as i32));
    let mut process = rd_examples::Process {
        child,
        #[cfg(target_os = "linux")]
        group,
    };
    tokio::select! {
        result = tokio::time::timeout(Duration::from_millis(deadlines.cell.max(1)), process.child.wait()) => match result {
            Ok(status) => Ok(status?),
            Err(_) => {
                rd_examples::terminate(&mut process, deadlines).await?;
                Err(failure("Native Python extraction timed out.".into()))
            }
        },
        () = cancellation.as_mut() => {
            rd_examples::terminate(&mut process, deadlines).await?;
            Err(AssemblyError::Cancelled)
        }
    }
}

fn collect_stub_docs(
    module: &str,
    parent: &str,
    declarations: &[python::ParsedDeclaration],
    docs: &mut Vec<serde_json::Value>,
) {
    for declaration in declarations {
        let name = format!("{parent}.{}", declaration.name);
        if let Some(doc) = &declaration.docstring {
            docs.push(serde_json::json!({ "module": module, "name": name, "kind": if declaration.kind == python::DeclarationKind::Class { "class" } else { "function" }, "text": doc.text }));
        }
        collect_stub_docs(module, &name, &declaration.members, docs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_the_platform_executable_from_a_relative_search_path() {
        let directory = tempfile::tempdir().unwrap();
        let bin = directory.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let executable = bin.join(format!("python{}", std::env::consts::EXE_SUFFIX));
        fs::write(&executable, []).unwrap();
        let search = std::env::join_paths(["missing", "bin"]).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(select_interpreter(directory.path(), &search), None);
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert_eq!(
            select_interpreter(directory.path(), &search),
            Some(executable)
        );
    }
}
