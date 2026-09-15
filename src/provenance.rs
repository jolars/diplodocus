//! Collect portable evidence from explicitly selected local inputs.
//!
//! Collection reads files and local Git metadata, never starts an extractor or
//! kernel, and never discovers a target's files. Producers supply complete file
//! selections through [`DeclaredSourceInputs`]. Missing selections keep the
//! repository fingerprint unknown. Metadata and declared environment files are
//! always collected. Runtime paths and file contents are not part of the result.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::configuration::{ExecutionConfigurationError, WorkspaceConfiguration};
use crate::configuration_validation::validate_configuration;
use crate::diagnostics::{Diagnostic, DiagnosticPath};
use crate::ir::{Fingerprint, InputFingerprint, Repository, TargetReference};
use crate::paths::{
    PathResolutionError, PathResolutionErrorKind, ResolvedRepositoryPaths, portable_relative_path,
    resolve_input_file, resolve_workspace_paths,
};

mod fingerprints;
mod observations;

pub use fingerprints::fingerprint_bytes;
pub use observations::*;

/// Exact in-process Panache dependency version, checked against the manifest.
pub const PANACHE_VERSION: &str = "0.29.0";

/// Component identities available to the authored adapter in this build.
///
/// Parser dependencies used only by extraction spikes do not claim production
/// observations. Future extractors provide the components they actually used.
pub fn builtin_tools() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("diplodocus".into(), env!("CARGO_PKG_VERSION").into()),
        ("panache-parser".into(), PANACHE_VERSION.into()),
    ])
}

/// Complete file enumerations supplied by source discovery or extraction.
///
/// The collector does not infer files, extensions, or targets. Omitted keys mean
/// unknown inputs; an explicit empty vector means observed empty inputs. Each
/// extractor must enumerate every contributing file, including references it
/// followed within its package. Vector order has no effect on portable output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclaredSourceInputs {
    /// Contributing files relative to each target's configured package root.
    pub extraction: BTreeMap<TargetReference, Vec<PathBuf>>,
    /// Selected authored files relative to each configured content root.
    pub content: BTreeMap<String, Vec<PathBuf>>,
}

/// Static evidence ready for a workspace assembler and later producers.
///
/// This collector result is not a new workspace wire schema. The assembler
/// places its existing IR records into the versioned workspace. No extraction
/// or execution success is claimed by collecting these inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaticProvenance {
    /// Repository IR, with declared revisions taking precedence over Git HEAD.
    pub repositories: BTreeMap<String, Repository>,
    /// Both revision sources retained independently for mismatch diagnostics.
    pub revisions: BTreeMap<String, RepositoryObservation>,
    /// Collected content hashes keyed by repository ID and relative path.
    pub inputs: BTreeMap<String, BTreeMap<DiagnosticPath, Fingerprint>>,
    /// Environment files by collection, sorted by repository and declared path.
    pub declared_environment_inputs: BTreeMap<String, Vec<InputFingerprint>>,
    /// Available in-process authored component identities and exact versions.
    pub tools: BTreeMap<String, String>,
}

/// A collection error; underlying runtime paths remain local troubleshooting data.
#[derive(Debug, Error)]
pub enum ProvenanceError {
    /// Invalid workspace identities or relationships.
    #[error("invalid workspace configuration")]
    Configuration {
        /// Deterministic portable validation diagnostics.
        diagnostics: Vec<Diagnostic>,
    },
    /// Incoherent execution declarations, without starting an engine.
    #[error("invalid execution declaration for collection `{collection}`: {source}")]
    Execution {
        /// Declaring collection ID.
        collection: String,
        /// Existing structured execution validation error.
        source: ExecutionConfigurationError,
    },
    /// Missing, invalid, escaping, or unreadable input.
    #[error(transparent)]
    Path(#[from] PathResolutionError),
    /// A producer selected an undeclared target or collection.
    #[error("input selection names undeclared {kind} `{id}`")]
    UnknownSelection {
        /// `target` or `collection`.
        kind: &'static str,
        /// Selected semantic identifier.
        id: String,
    },
    /// Repeated reads of one portable input disagreed during this collection.
    #[error("input changed during collection: `{repository}` `{}`", path.as_str())]
    InputChanged {
        /// Repository ID.
        repository: String,
        /// Portable source path.
        path: DiagnosticPath,
    },
}

/// Collect declared metadata, environment files, selected sources, and Git state.
///
/// Resolve configuration paths first, then recheck containment on every read.
/// Metadata and selected source locations name canonical repository-relative
/// referents. Environment records retain normalized declared paths for the
/// execution-cache contract. A declaration whose lexical normalization changes
/// its symlink/parent traversal is rejected rather than naming different bytes.
///
/// Hash exact file bytes with SHA-256. A complete repository fingerprint hashes
/// the ordered manifest described in [`fingerprint_bytes`], independent of Git
/// state, input enumeration order, and absolute roots. This is a static read,
/// not an atomic filesystem snapshot or an execution-cache implementation.
///
/// # Errors
///
/// Return configuration errors before selection errors, then path/read errors
/// in declaration order (selected files are sorted). Missing Git metadata or
/// failed Git observations remain unknown and do not fail collection.
pub fn collect_static_provenance(
    configuration_path: impl AsRef<Path>,
    configuration: &WorkspaceConfiguration,
    selections: &DeclaredSourceInputs,
) -> Result<StaticProvenance, ProvenanceError> {
    let configuration_path = configuration_path.as_ref();
    let diagnostics = validate_configuration(configuration);
    if !diagnostics.is_empty() {
        return Err(ProvenanceError::Configuration { diagnostics });
    }
    for collection in &configuration.content {
        collection
            .validate_execution()
            .map_err(|source| ProvenanceError::Execution {
                collection: collection.id.clone(),
                source,
            })?;
    }
    validate_selections(configuration, selections)?;
    let resolved = resolve_workspace_paths(configuration_path, configuration)?;
    let mut result = StaticProvenance {
        repositories: BTreeMap::new(),
        revisions: BTreeMap::new(),
        inputs: BTreeMap::new(),
        declared_environment_inputs: BTreeMap::new(),
        tools: builtin_tools(),
    };
    let mut complete = vec![true; resolved.repositories.len()];
    for repository in &resolved.repositories {
        result.inputs.insert(repository.id.clone(), BTreeMap::new());
    }
    for package in &resolved.packages {
        let repository = &resolved.repositories[package.repository_index];
        result.insert(read_input(
            configuration_path,
            repository,
            &package.metadata_path,
            None,
        )?)?;
        for target in &package.targets {
            let key = TargetReference {
                package: package.id.clone(),
                target: target.id.clone(),
            };
            if let Some(files) = selections.extraction.get(&key) {
                for file in sorted(files) {
                    let path = resolve_input_file(
                        configuration_path,
                        "extraction.inputs",
                        file,
                        &package.path,
                    )?;
                    result.insert(read_input(configuration_path, repository, &path, None)?)?;
                }
            } else {
                complete[package.repository_index] = false;
            }
        }
    }
    for (collection, declaration) in resolved.content.iter().zip(&configuration.content) {
        let repository = &resolved.repositories[collection.repository_index];
        if let Some(files) = selections.content.get(&collection.id) {
            for file in sorted(files) {
                let path = resolve_input_file(
                    configuration_path,
                    "content.inputs",
                    file,
                    &collection.path,
                )?;
                result.insert(read_input(configuration_path, repository, &path, None)?)?;
            }
        } else {
            complete[collection.repository_index] = false;
        }
        let mut environment = BTreeMap::new();
        for declared in sorted(&declaration.execution.declared_environment_inputs) {
            let field = "content.execution.declared_environment_inputs";
            let path = resolve_input_file(configuration_path, field, declared, &repository.path)?;
            let portable = portable_relative_path(configuration_path, field, declared)?;
            let normalized = resolve_input_file(
                configuration_path,
                field,
                Path::new(portable.as_str()),
                &repository.path,
            )?;
            if normalized != path {
                return Err(PathResolutionError {
                    configuration_path: configuration_path.to_owned(),
                    field: field.into(),
                    kind: PathResolutionErrorKind::InvalidPath {
                        path: declared.to_owned(),
                        reason: "normalization changes the declared symlink/parent traversal",
                    },
                }
                .into());
            }
            let input = read_input(
                configuration_path,
                repository,
                &path,
                Some(portable.clone()),
            )?;
            result.insert(input.clone())?;
            environment.insert(portable, input);
        }
        result
            .declared_environment_inputs
            .insert(collection.id.clone(), environment.into_values().collect());
    }
    for (index, (repository, declaration)) in resolved
        .repositories
        .iter()
        .zip(&configuration.repositories)
        .enumerate()
    {
        let observation = observe_repository(repository, declaration.revision.as_deref());
        result.repositories.insert(
            repository.id.clone(),
            Repository {
                canonical_url: declaration.url.clone(),
                source_link_template: declaration.source_link_template.clone(),
                revision: observation
                    .declared
                    .clone()
                    .or_else(|| observation.observed.clone()),
                dirty: observation.dirty,
                declared_input_fingerprint: complete[index].then(|| {
                    fingerprints::fingerprint_manifest(
                        &repository.id,
                        &result.inputs[&repository.id],
                    )
                }),
            },
        );
        result.revisions.insert(repository.id.clone(), observation);
    }
    Ok(result)
}

impl StaticProvenance {
    fn insert(&mut self, input: InputFingerprint) -> Result<(), ProvenanceError> {
        let inputs = self
            .inputs
            .get_mut(&input.source.repository)
            .expect("declared repository");
        if let Some(previous) = inputs.insert(input.source.path.clone(), input.fingerprint.clone())
            && previous != input.fingerprint
        {
            return Err(ProvenanceError::InputChanged {
                repository: input.source.repository,
                path: input.source.path,
            });
        }
        Ok(())
    }
}

fn sorted(files: &[PathBuf]) -> Vec<&PathBuf> {
    let mut files: Vec<_> = files.iter().collect();
    files.sort();
    files.dedup();
    files
}

fn validate_selections(
    configuration: &WorkspaceConfiguration,
    selections: &DeclaredSourceInputs,
) -> Result<(), ProvenanceError> {
    for key in selections.extraction.keys() {
        if !configuration.packages.iter().any(|package| {
            package.id == key.package
                && package.targets.iter().any(|target| target.id == key.target)
        }) {
            return Err(ProvenanceError::UnknownSelection {
                kind: "target",
                id: format!("{}::{}", key.package, key.target),
            });
        }
    }
    for key in selections.content.keys() {
        if !configuration
            .content
            .iter()
            .any(|collection| collection.id == *key)
        {
            return Err(ProvenanceError::UnknownSelection {
                kind: "collection",
                id: key.clone(),
            });
        }
    }
    Ok(())
}

fn read_input(
    configuration_path: &Path,
    repository: &ResolvedRepositoryPaths,
    path: &Path,
    declared: Option<DiagnosticPath>,
) -> Result<InputFingerprint, ProvenanceError> {
    let mut source = repository.source_location(path, None)?;
    // Read the just-validated canonical referent, not an unchecked declaration.
    let canonical = repository.path.join(source.path.as_str());
    let bytes = fs::read(&canonical).map_err(|source| PathResolutionError {
        configuration_path: configuration_path.to_owned(),
        field: "provenance.inputs".into(),
        kind: PathResolutionErrorKind::FileSystem {
            path: canonical,
            source,
        },
    })?;
    if let Some(path) = declared {
        source.path = path;
    }
    Ok(InputFingerprint {
        source,
        fingerprint: fingerprint_bytes(&bytes),
    })
}
