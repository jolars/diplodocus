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
