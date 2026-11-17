//! Native R documentation generation and verified checkout discovery.

use super::*;
use crate::configuration::{RDocumentationMode, RPackageConfiguration};
use crate::diagnostics::DiagnosticEntity;
use crate::extractors::r::{self, NativeObservation, RExtraction};
use crate::ir::{InputFingerprint, Provenance, ProvenanceActivity, RMetadata};
use crate::paths::{ResolvedPackagePaths, ResolvedRepositoryPaths};
use serde::{Deserialize, Serialize};
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
    dependencies: Vec<crate::incremental::Manifest>,
}

impl RObservation {
    pub(super) fn dependency_roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.dependencies
            .iter()
            .flat_map(|manifest| manifest.roots())
    }
    pub(super) fn dependency_library_roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.dependencies
            .iter()
            .flat_map(|manifest| manifest.native_library_roots())
    }
    pub(super) fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.inputs.keys().chain(
            self.dependencies
                .iter()
                .flat_map(|manifest| manifest.paths()),
        )
    }

    pub(super) fn verify(&self) -> Result<(), AssemblyError> {
        if inventory(&self.repository, &self.root, &self.extra_roots)? != self.inputs
            || self.dependencies.iter().any(|manifest| !manifest.verify())
        {
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
    #[serde(skip)]
    environment: Option<Vec<crate::incremental::Manifest>>,
    #[serde(skip)]
    cache_refusal: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Generation {
    roxygen2_version: String,
    loader: String,
    roclets: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheEnvironment {
    version: String,
    home: PathBuf,
    launcher: PathBuf,
    libraries: Vec<PathBuf>,
    packages: BTreeMap<String, PathBuf>,
    native: NativeEnvironment,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeEnvironment {
    complete: bool,
    #[serde(default)]
    reasons: Vec<String>,
    mapped: Vec<PathBuf>,
    loader_paths: Vec<PathBuf>,
    libraries: Vec<PathBuf>,
    namespaces: BTreeMap<String, PathBuf>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CachedGeneration {
    dependencies: crate::incremental::Manifest,
    resolutions: BTreeMap<String, Option<PathBuf>>,
    native: Vec<crate::incremental::Manifest>,
    provenance: Provenance,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CachedDiscovery {
    dependencies: crate::incremental::Manifest,
    resolutions: BTreeMap<String, Option<PathBuf>>,
    native: Vec<crate::incremental::Manifest>,
    extractions: BTreeMap<String, RExtraction>,
    runtime: Provenance,
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
    for index in crate::incremental::package_order(configuration, paths) {
        let declared = &configuration.packages[index];
        let package = &paths.packages[index];
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
        let cache_timing = reporting.phase(Phase::CacheValidation, Some(&package.id), None);
        let store = (declared.incremental.cache != crate::configuration::IncrementalCache::Off)
            .then(|| crate::incremental::Store::new(&paths.configuration_directory))
            .flatten();
        let supplemental = store.as_ref().and_then(|_| {
            crate::incremental::supplemental_manifest(configuration, paths, declared).ok()
        });
        let build = store
            .as_ref()
            .and_then(|_| crate::incremental::BuildIdentity::observe().ok());
        let environment = if store.is_some() && supplemental.is_some() && build.is_some() {
            cache_environment(package, deadlines, cancellation).await?
        } else {
            None
        };
        let identity = |inputs: &BTreeMap<PathBuf, Fingerprint>| {
            Some(crate::incremental::key(&(
                "r-package-preparation-v1",
                &package.path,
                declared,
                build.as_ref()?,
                supplemental.as_ref()?,
                environment.as_ref()?.identity(),
                deadlines.cell,
                inputs,
            )))
        };
        let cached_generation = if generate {
            store
                .as_ref()
                .zip(identity(&before))
                .and_then(|(store, key)| store.load::<CachedGeneration>("r-generation", &key))
                .filter(|cached| {
                    let valid = cached.dependencies.verify()
                        && cached.native.iter().all(|inputs| inputs.verify())
                        && environment
                            .as_ref()
                            .is_some_and(|environment| environment.resolves(&cached.resolutions));
                    if !valid && let Some((store, key)) = store.as_ref().zip(identity(&before)) {
                        let _ = store.discard("r-generation", &key);
                    }
                    valid
                })
        } else {
            None
        };
        if generate {
            reporting.cache(
                "r-generation",
                &package.id,
                cached_generation.is_some(),
                cached_generation
                    .is_none()
                    .then_some("inputs changed or no valid completed entry"),
            );
        }
        if (!generate || cached_generation.is_some())
            && let Some(cached) = store
                .as_ref()
                .zip(identity(&before))
                .and_then(|(store, key)| store.load::<CachedDiscovery>("r-discovery", &key))
                .filter(|cached| {
                    let valid = cached.dependencies.verify()
                        && cached.native.iter().all(|inputs| inputs.verify())
                        && environment
                            .as_ref()
                            .is_some_and(|environment| environment.resolves(&cached.resolutions))
                        && cached.extractions.len() == targets.len()
                        && targets
                            .iter()
                            .all(|target| cached.extractions.contains_key(&target.id));
                    if !valid && let Some((store, key)) = store.as_ref().zip(identity(&before)) {
                        let _ = store.discard("r-discovery", &key);
                    }
                    valid
                })
        {
            let mut dependencies = vec![cached.dependencies];
            dependencies.extend(cached.native);
            if let Some(generated) = &cached_generation
                && !dependencies.contains(&generated.dependencies)
            {
                dependencies.push(generated.dependencies.clone());
            }
            if let Some(generated) = &cached_generation {
                for manifest in &generated.native {
                    if !dependencies.contains(manifest) {
                        dependencies.push(manifest.clone());
                    }
                }
            }
            let observation = RObservation {
                repository: repository.clone(),
                root: package.path.clone(),
                extra_roots,
                inputs: before,
                dependencies,
            };
            observation.verify()?;
            if supplemental
                .as_ref()
                .is_some_and(|manifest| !manifest.verify())
            {
                return Err(AssemblyError::InputsChanged);
            }
            reporting.cache("r-discovery", &package.id, true, None);
            cache_timing.finish();
            let mut completed = cached_generation
                .into_iter()
                .map(|cached| cached.provenance)
                .chain(std::iter::once(cached.runtime))
                .collect();
            for (target, extraction) in cached.extractions {
                results.insert(
                    (package.id.clone(), target),
                    RNativeExtraction {
                        extraction,
                        completed: std::mem::take(&mut completed),
                        observation: observation.clone(),
                    },
                );
            }
            continue;
        }
        reporting.cache(
            "r-discovery",
            &package.id,
            false,
            Some("inputs changed or no valid completed entry"),
        );
        cache_timing.finish();
        let run_generation = generate && cached_generation.is_none();
        let mut intent = None;
        let native = run(
            package,
            repository,
            &descriptor,
            run_generation,
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
            run_generation,
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
            || native.generation.is_some() != run_generation
        {
            return Err(failure(
                &package.id,
                "Native R helper returned inconsistent completion evidence.",
            ));
        }
        let mut dependencies = environment
            .as_ref()
            .and_then(|environment| {
                dependency_manifest(
                    environment,
                    &metadata,
                    &native.observation,
                    &BTreeMap::new(),
                )
            })
            .map(|(manifest, _)| manifest)
            .into_iter()
            .collect::<Vec<_>>();
        if let Some(native) = &native.environment {
            dependencies.extend(native.iter().cloned());
        }
        if let Some(generated) = &cached_generation
            && !dependencies.contains(&generated.dependencies)
        {
            dependencies.push(generated.dependencies.clone());
        }
        if let Some(generated) = &cached_generation {
            for manifest in &generated.native {
                if !dependencies.contains(manifest) {
                    dependencies.push(manifest.clone());
                }
            }
        }
        let observation = RObservation {
            repository: repository.clone(),
            root: package.path.clone(),
            extra_roots,
            inputs: after.clone(),
            dependencies,
        };
        let runtime_inputs = fingerprints(repository, &after)?;
        let mut tools = BTreeMap::from([
            ("diplodocus".into(), env!("CARGO_PKG_VERSION").into()),
            ("r".into(), native.observation.r_version.clone()),
            ("pkgload".into(), native.observation.pkgload_version.clone()),
        ]);
        let mut completed: Vec<_> = cached_generation
            .into_iter()
            .map(|cached| cached.provenance)
            .collect();
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
        let mut extractions = BTreeMap::new();
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
            extractions.insert(target.id.clone(), extraction.clone());
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
        if supplemental
            .as_ref()
            .is_some_and(|manifest| !manifest.verify())
        {
            return Err(AssemblyError::InputsChanged);
        }
        if let (Some(store), Some(key), Some(environment)) =
            (&store, identity(&after), &environment)
            && supplemental.as_ref().is_some_and(|inputs| inputs.verify())
            && extractions.values().all(|result| {
                result
                    .diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.severity != Severity::Error)
            })
            && let Some((dependencies, resolutions)) =
                dependency_manifest(environment, &metadata, &native.observation, &extractions)
            && let Some(native_inputs) = &native.environment
            && native_inputs.iter().all(|inputs| inputs.verify())
        {
            if run_generation
                && let Some(provenance) = results
                    .values()
                    .filter(|result| result.extraction.package == package.id)
                    .flat_map(|result| &result.completed)
                    .find(|record| {
                        matches!(record.activity, ProvenanceActivity::RDocumentation { .. })
                    })
            {
                save_cache(
                    store,
                    "r-generation",
                    &key,
                    &CachedGeneration {
                        dependencies: dependencies.clone(),
                        resolutions: resolutions.clone(),
                        native: native_inputs.clone(),
                        provenance: provenance.clone(),
                    },
                    reporting,
                );
            }
            if let Some(runtime) = results
                .values()
                .filter(|result| result.extraction.package == package.id)
                .flat_map(|result| &result.completed)
                .find(|record| matches!(record.activity, ProvenanceActivity::RRuntime { .. }))
            {
                save_cache(
                    store,
                    "r-discovery",
                    &key,
                    &CachedDiscovery {
                        dependencies,
                        resolutions,
                        native: native_inputs.clone(),
                        extractions,
                        runtime: runtime.clone(),
                    },
                    reporting,
                );
            }
        } else if store.is_some() {
            let reason = native
                .cache_refusal
                .as_deref()
                .unwrap_or("native dependency coverage could not be verified");
            reporting.emit(crate::reporting::Event::Warning(&format!(
                "R preparation for `{}` could not be cached: {reason}.",
                package.id
            )));
        }
    }
    Ok(results)
}

fn save_cache<T: Serialize>(
    store: &crate::incremental::Store,
    stage: &str,
    key: &Fingerprint,
    value: &T,
    reporting: &Reporting,
) {
    if store.save(stage, key, value).is_err() {
        reporting.emit(crate::reporting::Event::Warning(
            "The completed R preparation could not be cached; continuing with its verified result.",
        ));
    }
}

fn dependency_manifest(
    environment: &CacheEnvironment,
    metadata: &RMetadata,
    observation: &NativeObservation,
    extractions: &BTreeMap<String, RExtraction>,
) -> Option<(
    crate::incremental::Manifest,
    BTreeMap<String, Option<PathBuf>>,
)> {
    let mut names: BTreeSet<_> = observation.dependencies.keys().cloned().collect();
    for dependencies in metadata.dependencies.values() {
        names.extend(
            dependencies
                .iter()
                .map(|dependency| dependency.name.clone()),
        );
    }
    for extraction in extractions.values() {
        if let ProvenanceActivity::Extraction { parsers, .. } = &extraction.provenance.activity {
            for parser in parsers.values() {
                names.extend(parser.settings.keys().filter_map(|key| {
                    key.strip_prefix("help:")
                        .or_else(|| key.strip_prefix("exports:"))
                        .map(str::to_owned)
                }));
            }
        }
    }
    installed_manifest(
        environment,
        names,
        &metadata.name,
        &observation.dependencies.keys().cloned().collect(),
    )
}

fn installed_manifest(
    environment: &CacheEnvironment,
    mut pending: BTreeSet<String>,
    own_name: &str,
    required: &BTreeSet<String>,
) -> Option<(
    crate::incremental::Manifest,
    BTreeMap<String, Option<PathBuf>>,
)> {
    let mut seen = BTreeSet::new();
    let mut resolutions = BTreeMap::new();
    let mut paths = vec![environment.launcher.clone()];
    paths.extend(native_inputs(&environment.native)?);
    while let Some(name) = pending.pop_first() {
        if name == own_name || name == "R" || !seen.insert(name.clone()) {
            continue;
        }
        // A new installation in an earlier library can shadow an unchanged dependency.
        paths.extend(
            environment
                .libraries
                .iter()
                .map(|library| library.join(&name)),
        );
        resolutions.insert(name.clone(), environment.packages.get(&name).cloned());
        let Some(path) = environment.packages.get(&name) else {
            if required.contains(&name) {
                return None;
            }
            continue;
        };
        let source = SourceLocation {
            repository: "environment".into(),
            path: "DESCRIPTION".try_into().ok()?,
            span: None,
        };
        let metadata = r::validate_metadata(
            &fs::read_to_string(path.join("DESCRIPTION")).ok()?,
            &source,
            &mut Vec::new(),
        )?;
        for field in ["Depends", "Imports", "LinkingTo"] {
            if let Some(dependencies) = metadata.dependencies.get(field) {
                pending.extend(
                    dependencies
                        .iter()
                        .map(|dependency| dependency.name.clone()),
                );
            }
        }
        paths.push(path.clone());
    }
    for directory in ["bin", "lib", "etc", "modules"] {
        let path = environment.home.join(directory);
        if path.exists() {
            paths.push(path);
        }
    }
    crate::incremental::Manifest::capture(
        &paths,
        &[
            "PATH",
            "R_LIBS",
            "R_LIBS_USER",
            "R_LIBS_SITE",
            "R_HOME",
            "R_DEFAULT_PACKAGES",
            "LANG",
            "LC_ALL",
            "LC_COLLATE",
            "LC_CTYPE",
            "LC_MESSAGES",
            "LC_TIME",
            "TZ",
            "LD_LIBRARY_PATH",
            "LD_PRELOAD",
            "LD_AUDIT",
            "DYLD_LIBRARY_PATH",
            "OMP_NUM_THREADS",
            "OPENBLAS_NUM_THREADS",
            "MKL_NUM_THREADS",
        ]
        .map(str::to_owned),
    )
    .ok()
    .map(|manifest| (manifest, resolutions))
}

fn native_inputs(environment: &NativeEnvironment) -> Option<Vec<PathBuf>> {
    if !environment.complete {
        return None;
    }
    let mut paths = environment.mapped.clone();
    paths.extend(environment.namespaces.values().cloned());
    for name in environment.namespaces.keys() {
        paths.extend(
            environment
                .libraries
                .iter()
                .map(|library| library.join(name)),
        );
    }
    paths.extend(["/etc/ld.so.cache", "/etc/ld.so.conf", "/etc/ld.so.conf.d"].map(PathBuf::from));
    let mapped: BTreeSet<_> = environment.mapped.iter().cloned().collect();
    let directories: BTreeSet<_> = environment
        .mapped
        .iter()
        .filter_map(|path| path.parent())
        .collect();
    for directory in directories {
        for entry in fs::read_dir(directory).ok()? {
            let entry = entry.ok()?;
            if entry.file_type().ok()?.is_symlink()
                && fs::canonicalize(entry.path()).is_ok_and(|target| mapped.contains(&target))
            {
                paths.push(entry.path());
            }
        }
    }
    Some(paths)
}

pub(crate) fn completed_environment_manifest(
    output: &Path,
) -> Option<Vec<crate::incremental::Manifest>> {
    completed_environment_receipt(output).ok()
}

fn completed_environment_receipt(
    output: &Path,
) -> Result<Vec<crate::incremental::Manifest>, String> {
    let mut path = output.as_os_str().to_os_string();
    path.push(".environment");
    let environment: NativeEnvironment = serde_json::from_slice(
        &fs::read(PathBuf::from(path))
            .map_err(|_| "native completion receipt is unavailable".to_owned())?,
    )
    .map_err(|_| "native completion receipt is malformed".to_owned())?;
    if !environment.complete {
        return Err(environment
            .reasons
            .first()
            .cloned()
            .unwrap_or_else(|| "native dependency coverage is uncertain".to_owned()));
    }
    let paths = native_inputs(&environment)
        .ok_or_else(|| "native dependency paths could not be observed".to_owned())?;
    Ok(vec![
        crate::incremental::Manifest::capture(&paths, &[])
            .map_err(|_| "native dependency files could not be observed".to_owned())?,
        crate::incremental::Manifest::capture_native_libraries(&environment.loader_paths, &[])
            .map_err(|_| "native loader paths could not be observed".to_owned())?,
    ])
}

pub(crate) async fn environment_manifest(
    package: &ResolvedPackagePaths,
    dependency_names: &[String],
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
) -> Result<Option<(Fingerprint, Vec<crate::incremental::Manifest>)>, AssemblyError> {
    let Some(environment) = cache_environment(package, deadlines, cancellation).await? else {
        return Ok(None);
    };
    let source = SourceLocation {
        repository: "environment".into(),
        path: "DESCRIPTION".try_into().unwrap(),
        span: None,
    };
    let Some(metadata) = r::validate_metadata(
        &fs::read_to_string(package.path.join("DESCRIPTION"))?,
        &source,
        &mut Vec::new(),
    ) else {
        return Ok(None);
    };
    let mut required: BTreeSet<_> = dependency_names.iter().cloned().collect();
    required.extend(["pkgload".into(), "jsonlite".into()]);
    let mut names = required.clone();
    for dependencies in metadata.dependencies.values() {
        names.extend(
            dependencies
                .iter()
                .map(|dependency| dependency.name.clone()),
        );
    }
    Ok(
        installed_manifest(&environment, names, &metadata.name, &required).and_then(
            |(manifest, resolutions)| {
                Some((
                    crate::incremental::key(&(environment.identity(), resolutions)),
                    vec![
                        manifest,
                        crate::incremental::Manifest::capture_native_libraries(
                            &environment.native.loader_paths,
                            &[],
                        )
                        .ok()?,
                    ],
                ))
            },
        ),
    )
}

impl CacheEnvironment {
    fn identity(&self) -> (&str, &Path, &Path, &[PathBuf], &NativeEnvironment) {
        (
            &self.version,
            &self.home,
            &self.launcher,
            &self.libraries,
            &self.native,
        )
    }

    fn resolves(&self, dependencies: &BTreeMap<String, Option<PathBuf>>) -> bool {
        dependencies
            .iter()
            .all(|(name, path)| self.packages.get(name) == path.as_ref())
    }
}

async fn cache_environment(
    package: &ResolvedPackagePaths,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
) -> Result<Option<CacheEnvironment>, AssemblyError> {
    let staging = tempfile::tempdir()?;
    let output = staging.path().join("environment.json");
    let helper = staging.path().join("environment.R");
    fs::write(
        &helper,
        format!(
            "{}\nobserve_cache_environment(commandArgs(trailingOnly = TRUE)[[1L]])\n",
            include_str!("r_environment.R")
        ),
    )?;
    let mut command = tokio::process::Command::new("R");
    command
        .args(["--vanilla", "--slave", "-f"])
        .arg(&helper)
        .arg("--args")
        .arg(&output)
        .current_dir(&package.path)
        .env("R_DEFAULT_PACKAGES", "NULL")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(target_os = "linux")]
    command.process_group(0);
    let Ok(child) = command.spawn() else {
        return Ok(None);
    };
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
                Ok(status) => status.ok(),
                Err(_) => {
                    rd_examples::terminate(&mut process, deadlines).await?;
                    None
                }
            }
        },
        () = cancellation.as_mut() => {
            rd_examples::terminate(&mut process, deadlines).await?;
            return Err(AssemblyError::Cancelled);
        }
    };
    process.kill();
    #[cfg(target_os = "linux")]
    {
        process.group = None;
    }
    let environment = status
        .filter(|status| status.success())
        .and_then(|_| fs::read(output).ok())
        .and_then(|bytes| serde_json::from_slice::<CacheEnvironment>(&bytes).ok())
        .filter(|environment| environment.native.complete);
    staging.close()?;
    Ok(environment)
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
                "{}\n{}\n{}",
                include_str!("r_runtime.R"),
                include_str!("r_environment.R"),
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
        match completed_environment_receipt(&output) {
            Ok(environment) => result.environment = Some(environment),
            Err(reason) => result.cache_refusal = Some(reason),
        }
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
