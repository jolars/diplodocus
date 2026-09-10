//! Typed parsing and loading of workspace configuration.
//!
//! These types retain declarations before semantic validation. Parsing checks
//! TOML syntax, field types, required fields, and supported enum values. It does
//! not resolve paths or references, check execution policy, or discover inputs.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::documents::AuthoredFormat;

/// The declarations in a root `diplodocus.toml` file, in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceConfiguration {
    /// Project-wide identity.
    pub project: ProjectConfiguration,
    /// Explicit local source repositories.
    #[serde(default, rename = "repository", skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<RepositoryConfiguration>,
    /// Packages and internal components to document.
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
    /// Declared extraction targets; never inferred from the ecosystem or files.
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

/// Declared execution settings, pending collection-level policy validation.
///
/// A parsed value alone does not authorize kernel discovery or execution.
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
/// Omitted collections are empty. Package kind, package visibility, and execution
/// mode use their documented defaults. Unknown fields are rejected in every table.
/// Paths and unresolved references retain their declared spelling.
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
/// fields, incorrect field types, or unsupported enum values. Semantic validation
/// of paths, identities, relationships, and execution policy is a separate step.
pub fn parse_configuration(source: &str) -> Result<WorkspaceConfiguration, toml::de::Error> {
    toml::from_str(source)
}

/// Read and parse one explicitly supplied workspace configuration file.
///
/// Only the configuration file is read. Declared paths remain unresolved, and
/// parsing does not discover repositories, packages, targets, or kernels.
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
