//! Native R documentation generation and verified checkout discovery.

use super::*;
use crate::configuration::{RDocumentationMode, RPackageConfiguration};
use crate::diagnostics::DiagnosticEntity;
use crate::extractors::r::{self, NativeObservation, RExtraction};
use crate::ir::{InputFingerprint, Provenance, ProvenanceActivity, RMetadata};
use crate::paths::{ResolvedPackagePaths, ResolvedRepositoryPaths};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::process::Stdio;
use std::time::Duration;

/// Exact native writes acknowledged by the preview watcher, including deletions.
pub(crate) type GeneratedWrites = BTreeMap<PathBuf, Option<Fingerprint>>;

/// Whether a nonreserved package path can be consumed by native R discovery.
pub(crate) fn native_source_path(package_root: &Path, path: &Path) -> bool {
    if crate::generated_storage::is_reserved(path) {
        return false;
    }
    let Ok(relative) = path.strip_prefix(package_root) else {
        return false;
    };
    if [
        "DESCRIPTION",
        "NAMESPACE",
        ".Rbuildignore",
        "man/roxygen/meta.R",
    ]
    .iter()
    .any(|native| relative == Path::new(native))
    {
        return true;
    }
    ["R", "man", "data"]
        .iter()
        .any(|directory| relative.starts_with(directory))
}

pub(super) struct RNativeExtraction {
    pub extraction: RExtraction,
    pub completed: Vec<Provenance>,
    pub observation: RObservation,
}

#[derive(Debug, Clone)]
pub(super) struct RObservation {
    repository: ResolvedRepositoryPaths,
    root: PathBuf,
    extra_roots: Vec<PathBuf>,
    inputs: BTreeMap<PathBuf, Fingerprint>,
}

impl RObservation {
    pub(super) fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.inputs.keys()
    }

    pub(super) fn verify(&self) -> Result<(), AssemblyError> {
        if inventory(&self.repository, &self.root, &self.extra_roots)? != self.inputs {
            return Err(AssemblyError::InputsChanged);
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeResult {
    complete: String,
    observation: NativeObservation,
    generation: Option<Generation>,
    #[serde(default)]
    warnings: Vec<String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Generation {
    roxygen2_version: String,
    loader: String,
    roclets: Vec<String>,
}

fn failure(package: &str, message: impl Into<String>) -> AssemblyError {
    AssemblyError::Diagnostics(vec![
        Diagnostic::new(DiagnosticCode::RMetadata, Severity::Error, message)
            .with_entity(DiagnosticEntity::Package { id: package.into() }),
    ])
}

pub(super) async fn results(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
    reporting: &Reporting,
    journal: &mut GeneratedWrites,
) -> Result<BTreeMap<(String, String), RNativeExtraction>, AssemblyError> {
    let mut results = BTreeMap::new();
    for (declared, package) in configuration.packages.iter().zip(&paths.packages) {
        let targets: Vec<_> = declared
            .targets
            .iter()
            .zip(&package.targets)
            .filter(|(target, _)| target.extractor == "r")
            .map(|(_, resolved)| resolved)
            .collect();
        if targets.is_empty() {
            continue;
        }
        // Generation owns package-relative metadata and cannot target a nested checkout.
        if targets.iter().any(|target| target.path != package.path) {
            return Err(failure(
                &package.id,
                "Native R targets must select their configured package root.",
            ));
        }
        let repository = &paths.repositories[package.repository_index];
        let mut options = declared.r.clone().unwrap_or_default();
        let source = repository.source_location(&package.metadata_path, None)?;
        let metadata_text = fs::read_to_string(&package.metadata_path)?;
        let mut diagnostics = vec![];
        let metadata = r::validate_metadata(&metadata_text, &source, &mut diagnostics);
        if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
        {
            return Err(AssemblyError::Diagnostics(diagnostics));
        }
        let metadata =
            metadata.ok_or_else(|| failure(&package.id, "Invalid R package metadata."))?;
        let descriptor_path = package.path.join("DESCRIPTION");
        let descriptor_source = repository.source_location(&descriptor_path, None)?;
        let descriptor_text = fs::read_to_string(&descriptor_path)?;
        let descriptor =
            r::validate_metadata(&descriptor_text, &descriptor_source, &mut diagnostics);
        if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
        {
            return Err(AssemblyError::Diagnostics(diagnostics));
        }
        let descriptor =
            descriptor.ok_or_else(|| failure(&package.id, "Invalid checkout DESCRIPTION."))?;
        if descriptor.name != metadata.name
            || !crate::validation::r_versions_equal(&descriptor.version, &metadata.version)
        {
            return Err(failure(
                &package.id,
                "Supplied checkout package identity does not match the configured DESCRIPTION.",
            ));
        }
        validate_declarations(package, &options)?;
        // Retargeting a declared alias must invalidate the native observation.
        let declared_outputs = options
            .generated_outputs
            .iter()
            .map(|path| package.path.join(path))
            .collect::<Vec<_>>();
        options.generated_outputs = options
            .generated_outputs
            .iter()
            .map(|output| {
                let resolved = resolved_output(package, output)?;
                Ok(resolved
                    .strip_prefix(&package.path)
                    .map_err(|_| AssemblyError::InputsChanged)?
                    .to_owned())
            })
            .collect::<Result<Vec<_>, AssemblyError>>()?;
        let mut extra_roots: Vec<_> = options
            .extra_inputs
            .iter()
            .chain(&options.generated_outputs)
            .map(|path| package.path.join(path))
            .collect();
        extra_roots.extend(declared_outputs);
        extra_roots.push(package.metadata_path.clone());
        let before = inventory(repository, &package.path, &extra_roots)?;
        if before.get(&package.metadata_path) != Some(&fingerprint_bytes(metadata_text.as_bytes()))
            || before.get(&descriptor_path) != Some(&fingerprint_bytes(descriptor_text.as_bytes()))
        {
            return Err(AssemblyError::InputsChanged);
        }
        let owned: BTreeSet<_> = before
            .keys()
            .filter(|path| roxygen_owned(path))
            .cloned()
            .collect();
        let generate = match options.documentation {
            RDocumentationMode::Auto => {
                descriptor.fields.contains_key("Roxygen")
                    || descriptor.fields.contains_key("RoxygenNote")
                    || descriptor
                        .fields
                        .keys()
                        .any(|key| key.starts_with("Config/roxygen2/"))
                    || package.path.join("man/roxygen/meta.R").is_file()
            }
            RDocumentationMode::Roxygen2 => true,
            RDocumentationMode::Rd => false,
        };
        let mut intent = None;
        let native = run(
            package,
            repository,
            &descriptor,
            generate,
            deadlines,
            cancellation,
            reporting,
            &mut intent,
        )
        .await;
        // A failed roclet can already have written output. Acknowledge only its allowed bytes.
        let after = inventory(repository, &package.path, &extra_roots)?;
        let collate = intent
            .as_ref()
            .is_some_and(|generation| generation.roclets.iter().any(|roclet| roclet == "collate"));
        let maintained_unchanged = acknowledge(
            repository,
            package,
            &options,
            &descriptor,
            &before,
            &after,
            generate,
            collate,
            &owned,
            journal,
        )?;
        let native = native?;
        if !maintained_unchanged {
            return Err(AssemblyError::InputsChanged);
        }
        if native.complete != "r-native-v1"
            || native.observation.package_name != metadata.name
            || !crate::validation::r_versions_equal(
                &native.observation.package_version,
                &metadata.version,
            )
            || native.generation.is_some() != generate
        {
            return Err(failure(
                &package.id,
                "Native R helper returned inconsistent completion evidence.",
            ));
        }
        let observation = RObservation {
            repository: repository.clone(),
            root: package.path.clone(),
            extra_roots,
            inputs: after.clone(),
        };
        let runtime_inputs = fingerprints(repository, &after)?;
        let mut tools = BTreeMap::from([
            ("diplodocus".into(), env!("CARGO_PKG_VERSION").into()),
            ("r".into(), native.observation.r_version.clone()),
            ("pkgload".into(), native.observation.pkgload_version.clone()),
        ]);
        let mut completed = vec![];
        if let Some(generation) = native.generation {
            tools.insert("roxygen2".into(), generation.roxygen2_version.clone());
            let outputs = after
                .iter()
                .filter(|(path, _)| {
                    generated_output(package, &options, path)
                        && (path.as_path() == package.path.join("DESCRIPTION")
                            || roxygen_owned(path)
                            || journal.contains_key(*path)
                            || options
                                .generated_outputs
                                .iter()
                                .any(|output| path.starts_with(package.path.join(output))))
                })
                .map(|(path, fingerprint)| (path.clone(), fingerprint.clone()))
                .collect();
            completed.push(Provenance {
                activity: ProvenanceActivity::RDocumentation {
                    package: package.id.clone(),
                    policy: "r-roxygen2-v1".into(),
                    r_version: native.observation.r_version.clone(),
                    roxygen2_version: generation.roxygen2_version,
                    loader: generation.loader,
                    roclets: generation.roclets,
                    dependencies: native.observation.dependencies.clone(),
                    inputs: fingerprints(repository, &before)?,
                    outputs: fingerprints(repository, &outputs)?,
                },
                source: None,
                span: None,
                tools: tools.clone(),
            });
        }
        completed.push(Provenance {
            activity: ProvenanceActivity::RRuntime {
                package: package.id.clone(),
                policy: "r-runtime-v1".into(),
                r_version: native.observation.r_version.clone(),
                package_version: native.observation.package_version.clone(),
                dependencies: native.observation.dependencies.clone(),
                inputs: runtime_inputs,
            },
            source: None,
            span: None,
            tools,
        });
        for target in targets {
            observation.verify()?;
            let timing = reporting.phase(Phase::Extraction, Some(&package.id), Some(&target.id));
            let mut extraction =
                r::extract_native_target(repository, package, target, &native.observation);
            extraction
                .diagnostics
                .extend(native.warnings.iter().map(|message| {
                    Diagnostic::new(DiagnosticCode::RMetadata, Severity::Warning, message)
                        .with_entity(DiagnosticEntity::Package {
                            id: package.id.clone(),
                        })
                }));
            timing.finish();
            results.insert(
                (package.id.clone(), target.id.clone()),
                RNativeExtraction {
                    extraction,
                    completed: std::mem::take(&mut completed),
                    observation: observation.clone(),
                },
            );
        }
        observation.verify()?;
    }
    Ok(results)
}

#[allow(clippy::too_many_arguments)]
async fn run(
    package: &ResolvedPackagePaths,
    repository: &ResolvedRepositoryPaths,
    metadata: &RMetadata,
    generate: bool,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
    reporting: &Reporting,
    intent: &mut Option<Generation>,
) -> Result<NativeResult, AssemblyError> {
    let staging = tempfile::tempdir()?;
    let result = async {
        let output = staging.path().join("native.json");
        let errors = staging.path().join("stderr");
        let helper = staging.path().join("native.R");
        fs::write(
            &helper,
            format!(
                "{}\n{}",
                include_str!("r_runtime.R"),
                include_str!("r_native.R")
            ),
        )?;
        let mut command = tokio::process::Command::new("R");
        command
            .args(["--vanilla", "--slave", "-f"])
            .arg(&helper)
            .arg("--args")
            .arg(&package.path)
            .arg(&output)
            .arg(&metadata.name)
            .arg(&metadata.version)
            .arg(if generate { "roxygen2" } else { "rd" })
            .current_dir(&package.path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(fs::File::create(&errors)?)
            .kill_on_drop(true);
        #[cfg(target_os = "linux")]
        command.process_group(0);
        let timing = reporting.phase(Phase::NativeProcess, Some(&package.id), None);
        let child = command.spawn().map_err(|_| failure(&package.id, "Native R discovery requires R, pkgload, jsonlite, and configured roxygen2 in the prepared environment."))?;
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
            result = tokio::time::timeout(Duration::from_millis(deadlines.cell.max(1)), process.child.wait()) => match result {
                Ok(status) => status.map_err(AssemblyError::Io),
                Err(_) => {
                    rd_examples::terminate(&mut process, deadlines).await?;
                    Err(failure(&package.id, "Native R documentation generation or discovery timed out."))
                }
            },
            () = cancellation.as_mut() => {
                rd_examples::terminate(&mut process, deadlines).await?;
                Err(AssemblyError::Cancelled)
            }
        };
        process.kill();
        #[cfg(target_os = "linux")]
        {
            process.group = None;
        }
        *intent = fs::read_to_string(output.with_extension("json.generation"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
        reporting.native_timings(&output.with_extension("json.timings"), &package.id, None);
        let status = status?;
        if !status.success() {
            let message = fs::read_to_string(&errors)
                .unwrap_or_default()
                .replace(repository.path.to_string_lossy().as_ref(), "<repository>")
                .replace(staging.path().to_string_lossy().as_ref(), "<r-native>");
            return Err(failure(
                &package.id,
                format!(
                    "Native R documentation generation or discovery failed: {}",
                    message.trim()
                ),
            ));
        }
        let text = fs::read_to_string(&output)?;
        let mut result: NativeResult = serde_json::from_str(&text).map_err(|_| {
            failure(
                &package.id,
                "Native R helper returned malformed completion evidence.",
            )
        })?;
        for message in result.warnings.iter_mut().chain(
            result
                .observation
                .bindings
                .iter_mut()
                .filter_map(|binding| binding.error.as_mut()),
        ) {
            *message = message.replace(repository.path.to_string_lossy().as_ref(), "<repository>")
                .replace(staging.path().to_string_lossy().as_ref(), "<r-native>");
        }
        timing.finish();
        Ok(result)
    }
    .await;
    match (result, staging.close()) {
        (result, Ok(())) => result,
        (Ok(_), Err(error)) => Err(error.into()),
        (Err(cause), Err(source)) => Err(AssemblyError::Cleanup {
            cause: Box::new(cause),
            source,
        }),
    }
}

fn validate_declarations(
    package: &ResolvedPackagePaths,
    options: &RPackageConfiguration,
) -> Result<(), AssemblyError> {
    let overlaps = |left: &Path, right: &Path| left.starts_with(right) || right.starts_with(left);
    let mut input_roots = Vec::new();
    for input in [
        "DESCRIPTION",
        "NAMESPACE",
        ".Rbuildignore",
        "renv.lock",
        "R",
        "src",
        "inst",
        "data",
        "man/roxygen",
        "man/templates",
        "man/macros",
    ] {
        input_roots.push(package.path.join(input));
    }
    for input in &options.extra_inputs {
        input_roots.push(contained(&package.path.join(input), &package.path)?);
    }
    for output in &options.generated_outputs {
        if [
            "DESCRIPTION",
            "NAMESPACE",
            ".Rbuildignore",
            "renv.lock",
            "R",
            "src",
            "inst",
            "data",
            "man/roxygen",
            "man/templates",
            "man/macros",
        ]
        .iter()
        .any(|input| overlaps(output, Path::new(input)))
            || options
                .extra_inputs
                .iter()
                .any(|input| overlaps(output, input))
        {
            return Err(failure(
                &package.id,
                "Custom R generated outputs must not overlap maintained package inputs or declared extra inputs.",
            ));
        }
    }
    for path in &options.generated_outputs {
        let resolved = resolved_output(package, path)?;
        if input_roots.iter().any(|input| overlaps(&resolved, input)) {
            return Err(failure(
                &package.id,
                "Custom R generated outputs must not alias maintained package inputs.",
            ));
        }
    }
    Ok(())
}

pub(super) fn resolved_output(
    package: &ResolvedPackagePaths,
    declared: &Path,
) -> Result<PathBuf, AssemblyError> {
    let absolute = package.path.join(declared);
    if fs::symlink_metadata(&absolute).is_ok() {
        return contained(&absolute, &package.path);
    }
    let mut ancestor = absolute.parent();
    while let Some(path) = ancestor {
        if fs::symlink_metadata(path).is_ok() {
            let canonical = contained(path, &package.path)?;
            return Ok(canonical.join(absolute.strip_prefix(path).expect("output ancestor")));
        }
        ancestor = path.parent();
    }
    Err(AssemblyError::InputsChanged)
}

fn inventory(
    repository: &ResolvedRepositoryPaths,
    root: &Path,
    extras: &[PathBuf],
) -> Result<BTreeMap<PathBuf, Fingerprint>, AssemblyError> {
    let mut files = BTreeMap::new();
    fn visit(
        repository: &ResolvedRepositoryPaths,
        boundary: &Path,
        root: &Path,
        files: &mut BTreeMap<PathBuf, Fingerprint>,
        seen: &mut BTreeSet<PathBuf>,
        ignore: bool,
    ) -> Result<(), AssemblyError> {
        if crate::generated_storage::is_reserved(root) {
            return Ok(());
        }
        let canonical = contained(root, boundary)?;
        if canonical.is_file() {
            let source = repository.source_location(&canonical, None)?;
            if crate::generated_storage::is_generated_file(&canonical) {
                if native_source_path(boundary, root) || native_source_path(boundary, &canonical) {
                    return Err(AssemblyError::Diagnostics(vec![Diagnostic::new(
                        DiagnosticCode::RMetadata,
                        Severity::Error,
                        format!("Generated Diplodocus storage at `{}` occupies a native R input path. Move the snapshot outside native inputs, such as `.diplodocus/`.", source.path.as_str()),
                    ).with_source(DiagnosticSource::Repository { repository: source.repository, path: source.path })]));
                }
                return Ok(());
            }
            let path = canonical;
            files.insert(path.clone(), fingerprint_bytes(&fs::read(path)?));
            return Ok(());
        }
        if !seen.insert(canonical.clone()) {
            return Ok(());
        }
        for entry in fs::read_dir(canonical)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if ignore
                && entry.path().is_dir()
                && (name.starts_with('.')
                    || matches!(
                        name.as_ref(),
                        "target" | "node_modules" | "site" | "artifacts"
                    ))
            {
                continue;
            }
            visit(repository, boundary, &entry.path(), files, seen, ignore)?;
        }
        Ok(())
    }
    for name in [
        "DESCRIPTION",
        "NAMESPACE",
        ".Rbuildignore",
        "renv.lock",
        "R",
        "src",
        "inst",
        "data",
        "man",
    ] {
        let path = root.join(name);
        if path.exists() {
            visit(
                repository,
                root,
                &path,
                &mut files,
                &mut BTreeSet::new(),
                true,
            )?;
        }
    }
    for extra in extras {
        if extra.exists() {
            visit(
                repository,
                root,
                extra,
                &mut files,
                &mut BTreeSet::new(),
                false,
            )?;
        }
    }
    Ok(files)
}

fn generated_output(
    package: &ResolvedPackagePaths,
    options: &RPackageConfiguration,
    path: &Path,
) -> bool {
    let Ok(relative) = path.strip_prefix(&package.path) else {
        return false;
    };
    relative == Path::new("DESCRIPTION")
        || relative == Path::new("NAMESPACE")
        || (relative.starts_with("man")
            && relative
                .extension()
                .is_some_and(|extension| extension == "Rd"))
        || relative.starts_with("man/figures")
        || options
            .generated_outputs
            .iter()
            .any(|output| relative.starts_with(output))
}

#[allow(clippy::too_many_arguments)]
fn acknowledge(
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
    options: &RPackageConfiguration,
    metadata: &RMetadata,
    before: &BTreeMap<PathBuf, Fingerprint>,
    after: &BTreeMap<PathBuf, Fingerprint>,
    generate: bool,
    collate: bool,
    owned: &BTreeSet<PathBuf>,
    journal: &mut GeneratedWrites,
) -> Result<bool, AssemblyError> {
    let keys: BTreeSet<_> = before.keys().chain(after.keys()).collect();
    let mut unchanged = true;
    for path in keys {
        if before.get(path) == after.get(path) {
            continue;
        }
        if !generate || !generated_output(package, options, path) {
            unchanged = false;
            continue;
        }
        let relative = path.strip_prefix(&package.path).expect("contained input");
        let declared = options
            .generated_outputs
            .iter()
            .any(|output| relative.starts_with(output));
        if !declared
            && (relative == Path::new("NAMESPACE")
                || relative
                    .extension()
                    .is_some_and(|extension| extension == "Rd"))
            && !owned.contains(path)
            && !roxygen_owned(path)
        {
            unchanged = false;
            continue;
        }
        if path == &package.path.join("DESCRIPTION") {
            let mut diagnostics = vec![];
            let source = repository.source_location(path, None)?;
            let parsed =
                r::validate_metadata(&fs::read_to_string(path)?, &source, &mut diagnostics);
            let maintained = |metadata: &RMetadata| {
                metadata
                    .fields
                    .iter()
                    .filter(|(name, _)| {
                        !matches!(name.as_str(), "RoxygenNote" | "Config/roxygen2/version")
                            && !(collate && name.as_str() == "Collate")
                    })
                    .map(|(name, field)| (name.clone(), field.value.clone()))
                    .collect::<BTreeMap<_, _>>()
            };
            if parsed
                .as_ref()
                .is_none_or(|parsed| maintained(parsed) != maintained(metadata))
                || !diagnostics.is_empty()
            {
                unchanged = false;
                continue;
            }
        }
        journal.insert(path.clone(), after.get(path).cloned());
    }
    Ok(unchanged)
}

fn fingerprints(
    repository: &ResolvedRepositoryPaths,
    inputs: &BTreeMap<PathBuf, Fingerprint>,
) -> Result<Vec<InputFingerprint>, AssemblyError> {
    let mut result = vec![];
    for (path, fingerprint) in inputs {
        let relative = path
            .strip_prefix(&repository.path)
            .map_err(|_| AssemblyError::InputsChanged)?;
        let path =
            crate::paths::portable_relative_path(&repository.path, "native R input", relative)?;
        result.push(InputFingerprint {
            source: SourceLocation {
                repository: repository.id.clone(),
                path,
                span: None,
            },
            fingerprint: fingerprint.clone(),
        });
    }
    // Component ordering differs from portable string ordering for sibling files
    // and directories, such as widget.h and widget/api.h.
    result.sort_by(|left, right| {
        (&left.source.repository, &left.source.path)
            .cmp(&(&right.source.repository, &right.source.path))
    });
    Ok(result)
}

fn roxygen_owned(path: &Path) -> bool {
    fs::read(path).is_ok_and(|bytes| {
        bytes.starts_with(b"% Generated by roxygen2:")
            || bytes.starts_with(b"# Generated by roxygen2:")
    })
}

fn contained(path: &Path, boundary: &Path) -> Result<PathBuf, AssemblyError> {
    let relative = path
        .strip_prefix(boundary)
        .map_err(|_| AssemblyError::InputsChanged)?;
    let relative = if relative.as_os_str().is_empty() {
        Path::new(".")
    } else {
        relative
    };
    Ok(crate::paths::resolve_repository_target(relative, boundary)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_fingerprints_follow_portable_path_order() {
        let repository = ResolvedRepositoryPaths {
            id: "repo".into(),
            path: PathBuf::from("/repository"),
        };
        let inputs = [
            "package/inst/include/widget/api.h",
            "package/inst/include/widget.h",
            "package/inst/include/widget-helper.h",
        ]
        .into_iter()
        .map(|path| {
            (
                repository.path.join(path),
                fingerprint_bytes(path.as_bytes()),
            )
        })
        .collect();
        let observed = fingerprints(&repository, &inputs).unwrap();
        assert_eq!(
            observed
                .iter()
                .map(|input| input.source.path.as_str())
                .collect::<Vec<_>>(),
            [
                "package/inst/include/widget-helper.h",
                "package/inst/include/widget.h",
                "package/inst/include/widget/api.h",
            ]
        );
    }

    #[test]
    fn native_storage_destinations_respect_r_inputs_and_reserved_paths() {
        let root = Path::new("/package");
        for path in [
            "DESCRIPTION",
            "NAMESPACE",
            ".Rbuildignore",
            "man/roxygen/meta.R",
            "R/export.R",
            "R/unix/export.r",
            "R/sysdata.rda",
            "R/subdir/site/index.html",
            "man/export.Rd",
            "man/export.sqlite",
            "data/export.RData",
            "data/export.rda",
            "data/export.R",
            "data/source.csv",
        ] {
            assert!(native_source_path(root, &root.join(path)), "{path}");
        }
        for path in [
            ".diplodocus/export.R",
            "R/.diplodocus/export.R",
            "R/.diplodocus-snapshot-export.R",
            "exports/snapshot.sqlite",
            "README.md",
        ] {
            assert!(!native_source_path(root, &root.join(path)), "{path}");
        }
    }
}
