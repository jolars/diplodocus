//! Portable package metadata and completed R author and citation information.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::SourceLocation;

/// Static DESCRIPTION metadata with the original fields and their evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RMetadata {
    /// Published R package name, independent of its workspace ID.
    pub name: String,
    /// Declared R package version.
    pub version: String,
    /// Short package title.
    pub title: String,
    /// Package description when supplied.
    pub description: Option<String>,
    /// Unevaluated license declaration.
    pub license: Option<String>,
    /// Dependencies grouped by their DESCRIPTION field, including R in Depends.
    pub dependencies: BTreeMap<String, Vec<RDependency>>,
    /// All original fields, including unevaluated Authors@R.
    pub fields: BTreeMap<String, RMetadataField>,
    /// Original metadata file.
    pub source: SourceLocation,
}

/// One DCF field, retaining folded text and its exact enclosing source range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RMetadataField {
    /// Continuation lines folded by the DCF parser.
    pub value: String,
    /// Exact source field.
    pub source: SourceLocation,
}

/// One static dependency declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RDependency {
    /// Package name, or R for the interpreter requirement.
    pub name: String,
    /// Constraints in declared order.
    pub constraints: Vec<RVersionConstraint>,
    /// Exact dependency source range.
    pub source: SourceLocation,
}

/// An unevaluated R version constraint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RVersionConstraint {
    /// One of <, <=, ==, >=, or >.
    pub operator: String,
    /// Version spelling in R syntax.
    pub version: String,
    /// Exact constraint source range.
    pub source: SourceLocation,
}

/// Ecosystem-specific metadata retained independently of API items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "language",
    content = "data",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum PackageMetadata {
    /// R DESCRIPTION and native author and citation results.
    R(RPackageMetadata),
    /// Static PEP 621 distribution metadata.
    Python(PythonMetadata),
    /// Static Julia Project.toml metadata.
    Julia(JuliaMetadata),
    /// Cargo-resolved package metadata, including workspace inheritance.
    Rust(RustMetadata),
}

/// Metadata from the supplied R checkout, never an unrelated installed release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RPackageMetadata {
    /// Original validated DESCRIPTION fields and their source evidence.
    pub description: RMetadata,
    /// Installed help candidates keyed by the referenced topic or package-qualified topic.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub help_links: BTreeMap<String, Vec<RHelpTarget>>,
    /// Results completed during extraction; absent during source validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RPackageRuntime>,
}

/// Installed help evidence retained for reference resolution without R.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RHelpTarget {
    /// Published package name.
    pub package: String,
    /// Canonical help filename, without an extension.
    pub topic: String,
    /// Repository used by rdrr.io.
    pub repository: RdrrRepository,
}

/// An rdrr.io help collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RdrrRepository {
    /// Packages bundled with R.
    R,
    /// CRAN packages.
    Cran,
    /// Bioconductor packages.
    Bioc,
}

/// Native R results stored so generation needs neither R nor the checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RPackageRuntime {
    /// People formatted by R, retaining contact information and contribution roles.
    pub authors: Vec<String>,
    /// Creators identified by R's contribution roles, used when Maintainer is absent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maintainers: Vec<String>,
    /// Native citation text and BibTeX, when authors or CITATION are supplied.
    pub citation: Option<RCitation>,
    /// R version observed while completing these results.
    pub r_version: String,
}

/// Native bibliography output retained as text rather than executable markup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RCitation {
    /// Human-readable citation, including multiple entries and explanatory text.
    pub text: String,
    /// BibTeX suitable for copying into a bibliography.
    pub bibtex: String,
}

/// Static PEP 621 metadata; dependencies use normalized PEP 508 spelling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonMetadata {
    /// Distribution name, independent of its configured documentation ID.
    pub name: String,
    /// PEP 440 distribution version.
    pub version: String,
    /// Optional short description.
    pub description: Option<String>,
    /// Declared license expression or inline license text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Maintained author names and contact information.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    /// Maintainer names and contact information.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maintainers: Vec<String>,
    /// Declared project links in deterministic label order.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub urls: BTreeMap<String, String>,
    /// Declared PEP 440 interpreter constraints.
    pub requires_python: Option<String>,
    /// Dependencies in metadata order.
    pub dependencies: Vec<String>,
    /// Optional dependency groups sorted by name.
    pub optional_dependencies: BTreeMap<String, Vec<String>>,
    /// Dynamic, unused metadata fields retained for diagnostics.
    pub dynamic: Vec<String>,
    /// Explicit grammar selected by the adapter, in `major.minor` form.
    pub target_version: String,
    /// File-level metadata evidence; the typed parser does not locate fields.
    pub source: SourceLocation,
}

/// Static Project.toml values; no manifest or Julia environment is consulted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JuliaMetadata {
    /// Published package name.
    pub name: String,
    /// Validated package UUID.
    pub uuid: String,
    /// Declared SemVer version when supplied.
    pub version: Option<String>,
    /// Maintained author names.
    pub authors: Vec<String>,
    /// Direct dependency UUIDs.
    pub dependencies: BTreeMap<String, String>,
    /// Julia/Pkg compatibility syntax retained without Cargo interpretation.
    pub compatibility: BTreeMap<String, String>,
    /// Every original top-level field with source evidence.
    pub fields: BTreeMap<String, JuliaMetadataField>,
    /// Portable metadata file location.
    pub source: SourceLocation,
}

/// A top-level metadata value and its exact TOML source range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JuliaMetadataField {
    /// Original value represented as portable JSON.
    pub value: serde_json::Value,
    /// Exact value range in the metadata file.
    pub source: SourceLocation,
}

/// Cargo-resolved package identity and presentation metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustMetadata {
    /// Published Cargo package name.
    pub name: String,
    /// Resolved package version.
    pub version: String,
    /// Short package description.
    pub description: Option<String>,
    /// Declared SPDX license expression.
    pub license: Option<String>,
    /// Maintained author names and contact information.
    pub authors: Vec<String>,
    /// Declared project links in deterministic label order.
    pub urls: BTreeMap<String, String>,
    /// Portable location of the package manifest.
    pub source: SourceLocation,
}

impl PackageMetadata {
    /// Published package name, independent of its workspace display name.
    pub fn name(&self) -> &str {
        match self {
            Self::R(metadata) => &metadata.description.name,
            Self::Python(metadata) => &metadata.name,
            Self::Julia(metadata) => &metadata.name,
            Self::Rust(metadata) => &metadata.name,
        }
    }

    /// Declared package version, when supplied by the ecosystem.
    pub fn version(&self) -> Option<&str> {
        match self {
            Self::R(metadata) => Some(&metadata.description.version),
            Self::Python(metadata) => Some(&metadata.version),
            Self::Julia(metadata) => metadata.version.as_deref(),
            Self::Rust(metadata) => Some(&metadata.version),
        }
    }

    /// Short introduction suitable for an overview without authored content.
    pub fn description(&self) -> Option<&str> {
        match self {
            Self::R(metadata) => metadata
                .description
                .description
                .as_deref()
                .or(Some(&metadata.description.title)),
            Self::Python(metadata) => metadata.description.as_deref(),
            Self::Julia(metadata) => metadata
                .fields
                .get("description")
                .and_then(|field| field.value.as_str()),
            Self::Rust(metadata) => metadata.description.as_deref(),
        }
    }

    /// Declared license expression or text, when supplied by the ecosystem.
    pub fn license(&self) -> Option<&str> {
        match self {
            Self::R(metadata) => metadata.description.license.as_deref(),
            Self::Python(metadata) => metadata.license.as_deref(),
            Self::Julia(metadata) => metadata
                .fields
                .get("license")
                .and_then(|field| field.value.as_str()),
            Self::Rust(metadata) => metadata.license.as_deref(),
        }
    }

    /// Metadata file from the declared checkout.
    pub fn source(&self) -> &SourceLocation {
        match self {
            Self::R(metadata) => &metadata.description.source,
            Self::Python(metadata) => &metadata.source,
            Self::Julia(metadata) => &metadata.source,
            Self::Rust(metadata) => &metadata.source,
        }
    }
}
