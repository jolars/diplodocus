//! Typed parsing and loading of workspace configuration.
//!
//! These types retain declarations before semantic validation. Parsing checks
//! TOML syntax, field types, required fields, supported enum values, and coherent
//! collection execution settings. It does not resolve filesystem paths or
//! references, validate document execution authority, or discover inputs.
//! Use [`crate::paths::resolve_workspace_paths`] for explicit filesystem validation.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::documents::AuthoredFormat;

/// The declarations in a root `diplodocus.toml` file, in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceConfiguration {
    /// Project-wide identity.
    pub project: ProjectConfiguration,
    /// Explicit local source repositories; omission declares none.
    #[serde(default, rename = "repository", skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<RepositoryConfiguration>,
    /// Explicit packages and internal components to document; omission declares none.
    #[serde(default, rename = "package", skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<PackageConfiguration>,
    /// Authored content collections.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<ContentConfiguration>,
    /// Explicit conceptual API groups.
    #[serde(default, rename = "concept", skip_serializing_if = "Vec::is_empty")]
    pub concepts: Vec<ConceptConfiguration>,
    /// Implementation and release relationships between packages.
    #[serde(
        default,
        rename = "relationship",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub relationships: Vec<RelationshipConfiguration>,
}

/// Project-wide documentation settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfiguration {
    /// Display name of the project.
    pub name: String,
}

/// An explicitly supplied local source repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryConfiguration {
    /// Stable repository identifier.
    pub id: String,
    /// Source root, relative to the configuration directory.
    pub path: PathBuf,
    /// Canonical source origin, when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Optional forge-specific template for revision, path, and line links.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_link_template: Option<String>,
    /// Explicit revision, including for sources without version-control metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

/// A package or internal component with explicit extraction targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageConfiguration {
    /// Stable package identity used by references and relationships.
    pub id: String,
    /// Display name of the package.
    pub name: String,
    /// Site-wide URL slug, independent of the ecosystem and published name.
    pub slug: String,
    /// Package ecosystem identifier.
    pub ecosystem: String,
    /// Identifier of the repository containing the package.
    pub repository: String,
    /// Package root, relative to its repository.
    pub path: PathBuf,
    /// Metadata file, relative to the package root.
    pub metadata_path: PathBuf,
    /// Distribution or component classification; defaults to `package`.
    #[serde(default)]
    pub kind: PackageKind,
    /// Navigation and search visibility; defaults to `public`.
    #[serde(default)]
    pub visibility: PackageVisibility,
    /// Explicit extraction targets; this field is required, but may be empty.
    /// An empty list disables API extraction for this package. Targets are never
    /// inferred from the ecosystem or files.
    pub targets: Vec<ExtractionTargetConfiguration>,
}

/// Whether a documentation unit represents a package or an internal component.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageKind {
    /// A distributable package.
    #[default]
    Package,
    /// An explicitly documented component.
    Component,
}

/// Visibility of a package in generated navigation and search.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageVisibility {
    /// Included in project navigation and search.
    #[default]
    Public,
    /// Addressable and searchable, but omitted from project navigation by default.
    Internal,
    /// Available as a semantic target without navigation or search entries.
    Hidden,
}

/// An explicit source target consumed by one API extractor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractionTargetConfiguration {
    /// Target identifier within its package.
    pub id: String,
    /// Selected extractor identifier, such as `python` or `r`.
    pub extractor: String,
    /// Source path, relative to the package root.
    pub path: PathBuf,
    /// Target role, such as `public-api` or `internal-api`.
    pub role: String,
}

/// An authored collection with independent ownership and source location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContentConfiguration {
    /// Stable collection identifier.
    pub id: String,
    /// Reserved owner `project`, or a package identifier.
    pub owner: String,
    /// Repository containing the authored sources.
    pub repository: String,
    /// Collection root, relative to its repository.
    pub path: PathBuf,
    /// URL mount beneath the owner's documentation root.
    pub mount: String,
    /// Explicit authored input profile.
    pub format: AuthoredFormat,
    /// Declared execution settings; omission defaults to `never`.
    #[serde(default)]
    pub execution: ExecutionConfiguration,
}

impl<'de> Deserialize<'de> for ContentConfiguration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Validation needs both the profile and the completed execution table,
        // regardless of their order in the source or the deserialization entry point.
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Declaration {
            id: String,
            owner: String,
            repository: String,
            path: PathBuf,
            mount: String,
            format: AuthoredFormat,
            #[serde(default)]
            execution: ExecutionConfiguration,
        }

        let declaration = Declaration::deserialize(deserializer)?;
        let collection = Self {
            id: declaration.id,
            owner: declaration.owner,
            repository: declaration.repository,
            path: declaration.path,
            mount: declaration.mount,
            format: declaration.format,
            execution: declaration.execution,
        };
        collection
            .validate_execution()
            .map_err(|error| D::Error::custom(format!("content `{}`: {error}", collection.id)))?;
        Ok(collection)
    }
}

impl ContentConfiguration {
    /// Validate the profile, mode, engine, kernel, and environment declarations together.
    ///
    /// Deserialization calls this automatically. Call it again after modifying
    /// a collection programmatically. This performs no filesystem reads, kernel
    /// discovery, or execution. Paths and kernel spelling remain as declared.
    ///
    /// # Errors
    ///
    /// Returns the first contradictory or malformed setting. Environment paths
    /// must name explicit repository-relative files and be lexically distinct.
    /// [`crate::paths::resolve_workspace_paths`] checks file existence, type, and
    /// symlink containment separately. Reading file contents remains a later step.
    pub fn validate_execution(&self) -> Result<(), ExecutionConfigurationError> {
        let execution = &self.execution;
        if execution.mode == ExecutionMode::Never {
            for (field, declared) in [
                ("engine", execution.engine.is_some()),
                ("kernel", execution.kernel.is_some()),
                (
                    "declared_environment_inputs",
                    !execution.declared_environment_inputs.is_empty(),
                ),
            ] {
                if declared {
                    return Err(ExecutionConfigurationError::InactiveSetting { field });
                }
            }
            return Ok(());
        }

        if self.format != AuthoredFormat::Qmd {
            return Err(ExecutionConfigurationError::GfmExecution);
        }
        if execution.engine != Some(ExecutionEngine::Jupyter) {
            return Err(ExecutionConfigurationError::MissingEngine);
        }
        let kernel = execution
            .kernel
            .as_deref()
            .ok_or(ExecutionConfigurationError::MissingKernel)?;
        if kernel.is_empty()
            || matches!(kernel, "." | "..")
            || !kernel
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
        {
            return Err(ExecutionConfigurationError::InvalidKernel);
        }

        let mut inputs = HashMap::new();
        for (index, path) in execution.declared_environment_inputs.iter().enumerate() {
            let components = environment_input_components(path).map_err(|reason| {
                ExecutionConfigurationError::InvalidEnvironmentInput { index, reason }
            })?;
            if let Some(first_index) = inputs.insert(components, index) {
                return Err(ExecutionConfigurationError::DuplicateEnvironmentInput {
                    index,
                    first_index,
                });
            }
        }
        Ok(())
    }
}

fn environment_input_components(path: &Path) -> Result<Vec<&str>, &'static str> {
    let path = path.to_str().ok_or("paths must be valid UTF-8")?;
    if path.is_empty() {
        return Err("paths must not be empty");
    }
    if path.starts_with('/')
        || (path.as_bytes().get(1) == Some(&b':') && path.as_bytes()[0].is_ascii_alphabetic())
    {
        return Err("paths must be relative to the collection's repository");
    }
    if path.contains('\\') {
        return Err("paths must use forward-slash separators");
    }
    if path.contains('\0') {
        return Err("paths must not contain NUL bytes");
    }
    if path.contains(['*', '?', '[', ']', '{', '}']) {
        return Err("declare individual files, not glob patterns");
    }
    if matches!(path.rsplit('/').next(), Some("" | "." | "..")) {
        return Err("declare a file, not a directory reference");
    }

    let mut components = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err("paths must not escape the collection's repository");
                }
            }
            component => components.push(component),
        }
    }
    Ok(components)
}

/// A contradictory or malformed collection execution declaration.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExecutionConfigurationError {
    /// A display-only collection has execution-specific settings.
    #[error("`{field}` requires `mode = \"execute\"`")]
    InactiveSetting {
        /// The contradictory execution field.
        field: &'static str,
    },
    /// A GFM collection requested execution.
    #[error("`mode = \"execute\"` requires `format = \"qmd\"`")]
    GfmExecution,
    /// Execution has no explicitly selected Jupyter engine.
    #[error("`mode = \"execute\"` requires `engine = \"jupyter\"`")]
    MissingEngine,
    /// Execution has no explicitly selected kernel.
    #[error("`mode = \"execute\"` requires an explicit kernel")]
    MissingKernel,
    /// The kernel selector is empty, a path, or outside the supported ASCII syntax.
    #[error(
        "invalid kernel selector: use ASCII letters, digits, `-`, `.`, or `_`, excluding empty names, `.` and `..`"
    )]
    InvalidKernel,
    /// An environment input does not declare an explicit repository-relative file.
    #[error("invalid `declared_environment_inputs[{index}]`: {reason}")]
    InvalidEnvironmentInput {
        /// Zero-based index of the invalid declaration.
        index: usize,
        /// Explanation of the violated path rule.
        reason: &'static str,
    },
    /// Two environment declarations normalize to the same relative path.
    #[error("`declared_environment_inputs[{index}]` duplicates input {first_index}")]
    DuplicateEnvironmentInput {
        /// Zero-based index of the duplicate declaration.
        index: usize,
        /// Zero-based index of the first declaration of that path.
        first_index: usize,
    },
}

/// Declared execution settings, validated in their owning collection's context.
///
/// Standalone deserialization checks field types. Collection deserialization also
/// checks their consistency through [`ContentConfiguration::validate_execution`].
/// Document authority and filesystem validation remain necessary before execution.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionConfiguration {
    /// Requested mode; defaults to `never`.
    #[serde(default)]
    pub mode: ExecutionMode,
    /// Selected execution engine, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<ExecutionEngine>,
    /// Explicit kernel selector, such as `python3` or `ir`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kernel: Option<String>,
    /// Environment manifests or lockfiles, relative to the collection's repository.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declared_environment_inputs: Vec<PathBuf>,
}

/// Workspace-level authored execution mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionMode {
    /// Keep cells display-only.
    #[default]
    Never,
    /// Request execution subject to the collection and document policies.
    Execute,
}

/// Supported authored execution engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionEngine {
    /// A page-scoped Jupyter kernel session.
    Jupyter,
}

/// An explicitly declared group of corresponding APIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConceptConfiguration {
    /// Stable concept identifier.
    pub id: String,
    /// How the member APIs correspond.
    pub kind: ConceptKind,
    /// Member declarations in source order, resolved during semantic validation.
    pub members: Vec<ConceptMember>,
}

/// How closely the APIs in a concept correspond.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConceptKind {
    /// Members represent the same API.
    Equivalent,
    /// Members provide comparable capabilities.
    Analogous,
    /// Members have an explicitly declared association.
    Related,
}

/// An unresolved item reference in a conceptual API group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConceptMember {
    /// Package containing the member.
    pub package: String,
    /// Semantic item ID or a qualified name that must resolve unambiguously.
    pub item: String,
}

/// An implementation or release relationship, distinct from an API concept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipConfiguration {
    /// Source workspace package ID or external ecosystem-qualified coordinate.
    pub from: String,
    /// Destination workspace package ID or external ecosystem-qualified coordinate.
    pub to: String,
    /// Typed implementation or release relationship.
    pub kind: RelationshipKind,
    /// Declared version constraint, retained in its ecosystem's syntax.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_constraint: Option<String>,
    /// Origin of the declaration, such as `explicit` or package metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
}

/// Supported implementation and release relationships between packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationshipKind {
    /// The source depends on the destination package.
    DependsOn,
    /// The source provides bindings to the destination package.
    Binds,
    /// The source wraps the destination package.
    Wraps,
    /// The source is generated from the destination package.
    GeneratedFrom,
}

/// A configuration load failure with its source path and underlying cause.
#[derive(Debug, Error)]
pub enum ConfigurationError {
    /// The configuration file could not be read as UTF-8 text.
    #[error("could not read configuration `{}`: {source}", path.display())]
    Read {
        /// Configuration path supplied by the caller.
        path: PathBuf,
        /// Underlying filesystem or UTF-8 error.
        source: std::io::Error,
    },
    /// The configuration is malformed or does not match the input schema.
    #[error("could not parse configuration `{}`: {source}", path.display())]
    Parse {
        /// Configuration path supplied by the caller.
        path: PathBuf,
        /// Underlying TOML error, including its source range when available.
        source: toml::de::Error,
    },
}

/// Parse declarations from TOML without reading any paths or executing code.
///
/// Omitted repository, package, and content collections are empty. Each declared
/// package requires a `targets` list; `targets = []` declares no API extraction.
/// Package kind, package visibility, and execution mode use their documented
/// defaults. Unknown fields are rejected in every table. Paths and unresolved
/// references retain their declared spelling.
///
/// ```
/// use diplodocus::configuration::parse_configuration;
///
/// let config = parse_configuration("[project]\nname = 'Foo'\n")?;
/// assert_eq!(config.project.name, "Foo");
/// assert!(config.packages.is_empty());
/// # Ok::<(), toml::de::Error>(())
/// ```
///
/// # Errors
///
/// Returns a TOML error for malformed syntax, missing required fields, unknown
/// fields, incorrect field types, unsupported enum values, or incoherent collection
/// execution settings. Filesystem, identity, relationship, and document-authority
/// validation remain separate steps.
pub fn parse_configuration(source: &str) -> Result<WorkspaceConfiguration, toml::de::Error> {
    toml::from_str(source)
}

/// Read and parse one explicitly supplied workspace configuration file.
///
/// Only the configuration file is read. Declared paths remain unresolved, and
/// parsing does not discover repositories, packages, targets, or kernels.
/// Use [`crate::paths::resolve_workspace_paths`] to resolve and validate declared
/// filesystem inputs after loading.
///
/// # Errors
///
/// Returns the source path and underlying error if reading or parsing fails.
pub fn load_configuration(
    path: impl AsRef<Path>,
) -> Result<WorkspaceConfiguration, ConfigurationError> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|source| ConfigurationError::Read {
        path: path.to_owned(),
        source,
    })?;
    parse_configuration(&source).map_err(|source| ConfigurationError::Parse {
        path: path.to_owned(),
        source,
    })
}
