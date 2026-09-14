//! Versioned, portable workspace records.
//!
//! Map keys are the authoritative entity IDs; records do not duplicate them.
//! Targets and items are keyed within a package. Maps and sets serialize in
//! sorted order; vectors retain semantic declaration or event order. This model
//! does not resolve references, assign IDs, inspect paths, or collect provenance.
//! Paths use the same validated spelling as diagnostics. A missing root path
//! means the declared repository or package root, never a machine checkout.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use crate::configuration::{
    ConceptKind, ExecutionEngine, ExecutionMode, PackageKind, PackageVisibility, RelationshipKind,
};
use crate::diagnostics::{Diagnostic, DiagnosticPath};
use crate::documents::AuthoredFormat;

use super::{Document, Fingerprint, Provenance, SourceSpan, SourcedSignature};

/// Current portable workspace schema version.
pub const WORKSPACE_SCHEMA_VERSION: u32 = 1;

/// The supported schema version, serialized as the integer `1`.
///
/// Deserialization rejects unsupported versions instead of interpreting another
/// schema as this one. The workspace version field is required even though a
/// newly constructed Rust workspace defaults to the current version.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SchemaVersion;

impl Serialize for SchemaVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(WORKSPACE_SCHEMA_VERSION)
    }
}

impl<'de> Deserialize<'de> for SchemaVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let version = u32::deserialize(deserializer)?;
        if version != WORKSPACE_SCHEMA_VERSION {
            return Err(serde::de::Error::custom(format!(
                "unsupported workspace IR schema version {version}; expected {WORKSPACE_SCHEMA_VERSION}"
            )));
        }
        Ok(Self)
    }
}

/// One coherent documentation snapshot from explicitly supplied repositories.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    /// Required wire-schema discriminator.
    pub schema_version: SchemaVersion,
    /// Project display name.
    pub name: String,
    /// Repositories keyed by workspace repository ID.
    pub repositories: BTreeMap<String, Repository>,
    /// Packages keyed by workspace package ID.
    pub packages: BTreeMap<String, Package>,
    /// Authored collections keyed by workspace collection ID.
    pub content_collections: BTreeMap<String, ContentCollection>,
    /// Pages keyed by semantic page ID, independent of rendered URLs.
    pub pages: BTreeMap<String, Page>,
    /// Concepts keyed by workspace concept ID.
    pub concepts: BTreeMap<String, Concept>,
    /// Relationships in declaration order.
    pub relationships: Vec<PackageRelationship>,
    /// Unique diagnostics in the common model's deterministic order.
    pub diagnostics: BTreeSet<Diagnostic>,
    /// Snapshot evidence in producer-defined source order.
    pub provenance: Vec<Provenance>,
}

/// Portable repository identity and observed revision information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    /// Canonical public origin, never a local checkout path.
    pub canonical_url: Option<String>,
    /// Forge-specific source-link template.
    pub source_link_template: Option<String>,
    /// Declared or observed revision when available.
    pub revision: Option<String>,
    /// Observed working-tree state; `None` means unknown.
    pub dirty: Option<bool>,
    /// Fingerprint of declared inputs when collected.
    pub declared_input_fingerprint: Option<Fingerprint>,
}

/// A documented package or internal component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    /// Site-wide URL slug, separate from the package ID.
    pub slug: String,
    /// Package display name.
    pub name: String,
    /// Ecosystem of the public API.
    pub ecosystem: String,
    /// Extracted package version, if available.
    pub version: Option<String>,
    /// Owning repository ID.
    pub repository: String,
    /// Repository-relative package root; `None` means the repository root.
    pub path: Option<DiagnosticPath>,
    /// Package-relative metadata file.
    pub metadata_path: DiagnosticPath,
    /// Package or component classification.
    pub kind: PackageKind,
    /// Navigation and search visibility.
    pub visibility: PackageVisibility,
    /// Extraction targets keyed by package-scoped target ID.
    pub extraction_targets: BTreeMap<String, ExtractionTarget>,
    /// API items keyed by package-scoped semantic item ID.
    pub items: BTreeMap<String, Item>,
}

/// One authoritative source consumed by an extractor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractionTarget {
    /// Selected extractor identifier.
    pub extractor: String,
    /// Package-relative root; `None` means the package root.
    pub path: Option<DiagnosticPath>,
    /// Target role, such as `public-api`.
    pub role: String,
}

/// Authored sources with independent ownership and repository location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentCollection {
    /// Project or package owner.
    pub owner: ContentOwner,
    /// Repository containing these sources.
    pub repository: String,
    /// Repository-relative root; `None` means the repository root.
    pub path: Option<DiagnosticPath>,
    /// URL mount relative to the owner's documentation root.
    pub mount: String,
    /// Explicit authored profile.
    pub format: AuthoredFormat,
    /// Portable declared execution settings.
    pub execution: ExecutionConfiguration,
}

/// Portable execution settings, separate from configuration's filesystem paths.
///
/// This is a declaration record, not execution authority. Collection policy and
/// document authority must still be validated before any execution.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionConfiguration {
    /// Requested execution mode.
    pub mode: ExecutionMode,
    /// Selected engine when enabled.
    pub engine: Option<ExecutionEngine>,
    /// Explicit kernel selector when enabled.
    pub kernel: Option<String>,
    /// Normalized repository-relative input files, sorted by path.
    pub declared_environment_inputs: BTreeSet<DiagnosticPath>,
}

/// Ownership does not depend on the repository containing a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ContentOwner {
    /// Project-wide documentation.
    Project,
    /// Documentation owned by one workspace package.
    Package {
        /// Workspace package ID.
        package: String,
    },
}

/// A page before URL assignment and rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    /// Project or package owner.
    pub owner: ContentOwner,
    /// Semantic origin of the page.
    pub kind: PageKind,
    /// Display title.
    pub title: String,
    /// Structured content and its own source evidence.
    pub document: SourcedDocument,
}

/// Semantic page categories, independent of site routes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PageKind {
    /// An authored collection page.
    Authored {
        /// Owning content collection ID.
        collection: String,
    },
    /// An API reference page.
    Api {
        /// Documented item.
        item: ItemReference,
    },
    /// A conceptual API group page.
    Concept {
        /// Workspace concept ID.
        concept: String,
    },
    /// A generated project or package overview.
    Overview,
}

/// Portable document envelope; the existing authored syntax tree is unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcedDocument {
    /// Structured content with document-relative byte spans.
    pub document: Document,
    /// Original authored or extractor-specific format.
    pub source_format: DocumentFormat,
    /// Original source location, which may differ from the item's definition.
    pub source_location: Option<SourceLocation>,
    /// Optional raw source retained for diagnostics.
    pub raw_source: Option<String>,
    /// Document-specific evidence in source order.
    pub provenance: Vec<Provenance>,
}

/// Provenance of a document's original syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DocumentFormat {
    /// An authored Markdown profile.
    Authored {
        /// Explicit input profile.
        format: AuthoredFormat,
    },
    /// Extracted documentation, such as `numpy-docstring` or `rd`.
    Extracted {
        /// Stable source-format identifier.
        name: String,
    },
    /// Structured content generated without a source document.
    Generated,
}

/// An extracted API entity, whose ID is its key in the package's item map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    /// Language-neutral item classification.
    pub kind: ItemKind,
    /// Unqualified display name.
    pub name: String,
    /// Language-qualified name; not assumed unique across overloads or aliases.
    pub qualified_name: String,
    /// Signatures in declaration order, each with independent source evidence.
    pub signatures: Vec<SourcedSignature>,
    /// Structured documentation when available.
    pub documentation: Option<SourcedDocument>,
    /// Definition location when available.
    pub source_location: Option<SourceLocation>,
    /// Child item IDs within this package, in declaration order.
    pub children: Vec<String>,
    /// Extraction evidence in source order.
    pub provenance: Vec<Provenance>,
}

/// Common API item categories; typed language extensions are a separate layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemKind {
    /// A module.
    Module,
    /// A callable function.
    Function,
    /// A method associated with a type or generic.
    Method,
    /// A type declaration.
    Type,
    /// A class declaration.
    Class,
    /// A named constant.
    Constant,
    /// A field or attribute.
    Field,
    /// A namespace.
    Namespace,
}

/// A package-scoped semantic item reference, independent of its rendered URL.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemReference {
    /// Workspace package ID.
    pub package: String,
    /// Extractor-assigned item ID within that package.
    pub item: String,
}

/// A package-scoped extraction target reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetReference {
    /// Workspace package ID.
    pub package: String,
    /// Target ID within that package.
    pub target: String,
}

/// A file and optional source range without a machine-specific root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceLocation {
    /// Workspace repository ID.
    pub repository: String,
    /// Normalized repository-relative path.
    pub path: DiagnosticPath,
    /// Zero-based, half-open source-file byte range when proven.
    pub span: Option<SourceSpan>,
}

/// An explicit group of corresponding APIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Concept {
    /// Degree of correspondence among the members.
    pub kind: ConceptKind,
    /// Resolved member identities, sorted by package and item ID.
    pub members: BTreeSet<ItemReference>,
    /// Optional conceptual documentation.
    pub documentation: Option<SourcedDocument>,
}

/// An implementation or release dependency between packages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRelationship {
    /// Relationship origin.
    pub from: PackageReference,
    /// Relationship destination.
    pub to: PackageReference,
    /// Relationship semantics.
    pub kind: RelationshipKind,
    /// Constraint retained in the destination ecosystem's syntax.
    pub version_constraint: Option<String>,
    /// Declaration or extraction evidence in source order.
    pub provenance: Vec<Provenance>,
}

/// Explicitly distinguishes workspace IDs from external package coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PackageReference {
    /// A package represented in this workspace.
    Workspace {
        /// Workspace package ID.
        package: String,
    },
    /// A package that need not have a supplied repository.
    External {
        /// External package ecosystem.
        ecosystem: String,
        /// Published package name in that ecosystem.
        name: String,
    },
}
