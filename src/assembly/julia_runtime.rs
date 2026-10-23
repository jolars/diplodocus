//! Native Julia checkout validation and completion run during extraction.

use super::*;
use crate::extractors::julia;
use std::process::Stdio;
use std::time::Duration;

pub(crate) async fn assemble_for_extraction(
    path: &Path,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
) -> Result<WorkspaceSources, AssemblyError> {
    let configuration = crate::configuration::load_configuration(path)?;
    reject_errors(validate_configuration_with_source(
        &configuration,
        "diplodocus.yaml".try_into().unwrap(),
    ))?;
    let paths = resolve_workspace_paths(path, &configuration)?;
    let mut results = BTreeMap::new();
    for package in &paths.packages {
        let repository = &paths.repositories[package.repository_index];
        let declaration = configuration
            .packages
            .iter()
            .find(|p| p.id == package.id)
            .unwrap();
        for (target, selected) in declaration.targets.iter().zip(&package.targets) {
            if target.extractor != "julia" {
                continue;
            }
            let mut result = julia::extract_target(repository, package, selected);
            reject_errors(
                result
                    .diagnostics
                    .iter()
                    .filter(|d| d.code != DiagnosticCode::JuliaUnresolvedDefinition)
                    .cloned()
                    .collect(),
            )?;
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
            fs::write(&helper, include_str!("../extractors/julia/runtime.jl"))?;
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
            if !status.success() {
                let message = fs::read_to_string(errors)?
                    .replace(repository.path.to_string_lossy().as_ref(), "<repository>")
                    .replace(staging.path().to_string_lossy().as_ref(), "<julia-helper>");
                return Err(failure(format!(
                    "Julia runtime extraction failed: {}",
                    message.trim()
                )));
            }
            let observation = toml::from_str(&fs::read_to_string(output)?)
                .map_err(|e| failure(format!("Invalid Julia runtime observations: {e}")))?;
            julia::runtime::reconcile(&mut result, observation).map_err(failure)?;
            reject_errors(result.diagnostics.clone())?;
            results.insert((package.id.clone(), target.id.clone()), result);
        }
    }
    let rust = super::rust_results(path, true, deadlines, cancellation).await?;
    super::assemble_with_native(path, results, rust)
}
