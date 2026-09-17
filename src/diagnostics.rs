//! Portable diagnostics shared by configuration and documentation processing.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ir::SourceSpan;

/// Stable diagnostic identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticCode {
    /// Authored syntax is outside Diplodocus's supported profile.
    #[serde(rename = "unsupported-authored-syntax")]
    UnsupportedAuthoredSyntax,
    /// Panache reported malformed embedded YAML.
    #[serde(rename = "invalid-embedded-yaml")]
    InvalidEmbeddedYaml,
    /// Cell-option precedence has more than one winner.
    #[serde(rename = "ambiguous-cell-option")]
    AmbiguousCellOption,
    /// Document metadata requests execution outside collection authority.
    #[serde(rename = "document-execution-not-authorized")]
    DocumentExecutionNotAuthorized,
    /// A QMD document declares an unsupported metadata setting.
    #[serde(rename = "unsupported-qmd-metadata")]
    UnsupportedQmdMetadata,
    /// A QMD metadata declaration has an invalid value type.
    #[serde(rename = "invalid-qmd-metadata")]
    InvalidQmdMetadata,
    /// A configuration file could not be read.
    #[serde(rename = "configuration-read-failed")]
    ConfigurationReadFailed,
    /// Configuration syntax or declarations do not match the input schema.
    #[serde(rename = "invalid-configuration")]
    InvalidConfiguration,
    /// A collection has contradictory or malformed execution settings.
    #[serde(rename = "invalid-execution-configuration")]
    InvalidExecutionConfiguration,
    /// A declared source path has an invalid spelling.
    #[serde(rename = "invalid-source-path")]
    InvalidSourcePath,
    /// A declared source path could not be inspected.
    #[serde(rename = "source-path-io")]
    SourcePathIo,
    /// A source path has the wrong filesystem type.
    #[serde(rename = "source-path-wrong-type")]
    SourcePathWrongType,
    /// A source path escapes its declared boundary.
    #[serde(rename = "source-path-outside-boundary")]
    SourcePathOutsideBoundary,
    /// A repository reference does not select exactly one declaration.
    #[serde(rename = "invalid-repository-reference")]
    InvalidRepositoryReference,
    /// A repository ID is declared more than once in the workspace.
    #[serde(rename = "duplicate-repository-id")]
    DuplicateRepositoryId,
    /// A package ID is declared more than once in the workspace.
    #[serde(rename = "duplicate-package-id")]
    DuplicatePackageId,
    /// An extraction target ID is declared more than once within a package.
    #[serde(rename = "duplicate-target-id")]
    DuplicateTargetId,
    /// A content collection ID is declared more than once in the workspace.
    #[serde(rename = "duplicate-content-id")]
    DuplicateContentId,
    /// A concept ID is declared more than once in the workspace.
    #[serde(rename = "duplicate-concept-id")]
    DuplicateConceptId,
    /// A package URL slug is declared more than once in the site.
    #[serde(rename = "duplicate-package-slug")]
    DuplicatePackageSlug,
    /// A content owner is neither `project` nor a declared package ID.
    #[serde(rename = "unknown-content-owner")]
    UnknownContentOwner,
    /// A concept member references an undeclared workspace package.
    #[serde(rename = "unknown-concept-package")]
    UnknownConceptPackage,
    /// An unqualified relationship endpoint references an undeclared package.
    #[serde(rename = "unknown-relationship-endpoint")]
    UnknownRelationshipEndpoint,
    /// An external relationship endpoint has malformed coordinate syntax.
    #[serde(rename = "invalid-external-package-coordinate")]
    InvalidExternalPackageCoordinate,
    /// Python package metadata is malformed or missing required static fields.
    #[serde(rename = "python-metadata")]
    PythonMetadata,
    /// Required Python package metadata needs backend execution.
    #[serde(rename = "python-dynamic-metadata")]
    PythonDynamicMetadata,
    /// Python source contains malformed syntax.
    #[serde(rename = "python-syntax")]
    PythonSyntax,
    /// Python syntax or version constraints exceed the supported target grammar.
    #[serde(rename = "python-unsupported-version")]
    PythonUnsupportedVersion,
    /// Python source has syntax outside the static extraction subset.
    #[serde(rename = "python-unsupported-syntax")]
    PythonUnsupportedSyntax,
    /// A declared Python input could not be read safely.
    #[serde(rename = "python-source-read")]
    PythonSourceRead,
    /// Multiple inputs define the same Python module surface.
    #[serde(rename = "python-module-collision")]
    PythonModuleCollision,
    /// A Python export list cannot be determined statically.
    #[serde(rename = "python-dynamic-export")]
    PythonDynamicExport,
    /// A public Python import cannot be resolved to a canonical entity.
    #[serde(rename = "python-unresolved-reexport")]
    PythonUnresolvedReexport,
    /// A maintained Python stub conflicts with its implementation.
    #[serde(rename = "python-conflicting-stub")]
    PythonConflictingStub,
    /// A public Python construct has unsupported semantics.
    #[serde(rename = "python-unsupported-surface")]
    PythonUnsupportedSurface,
    /// Python declarations have conflicting canonical identities.
    #[serde(rename = "python-duplicate-identity")]
    PythonDuplicateIdentity,
    /// A Python lookup name refers to multiple canonical entities.
    #[serde(rename = "python-conflicting-alias")]
    PythonConflictingAlias,
    /// A Python declaration lacks supported semantic identity inputs.
    #[serde(rename = "python-invalid-identity")]
    PythonInvalidIdentity,
    /// A Python docstring contains incomplete section syntax.
    #[serde(rename = "python-incomplete-docstring")]
    PythonIncompleteDocstring,
    /// A Python docstring contains unsupported markup or sections.
    #[serde(rename = "python-unsupported-docstring")]
    PythonUnsupportedDocstring,
    /// Decoded Python documentation cannot be mapped exactly to source.
    #[serde(rename = "python-docstring-source-attribution")]
    PythonDocstringSourceAttribution,
}

impl DiagnosticCode {
    /// Stable serialized identifier, also used to order diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedAuthoredSyntax => "unsupported-authored-syntax",
            Self::InvalidEmbeddedYaml => "invalid-embedded-yaml",
            Self::AmbiguousCellOption => "ambiguous-cell-option",
            Self::DocumentExecutionNotAuthorized => "document-execution-not-authorized",
            Self::UnsupportedQmdMetadata => "unsupported-qmd-metadata",
            Self::InvalidQmdMetadata => "invalid-qmd-metadata",
            Self::ConfigurationReadFailed => "configuration-read-failed",
            Self::InvalidConfiguration => "invalid-configuration",
            Self::InvalidExecutionConfiguration => "invalid-execution-configuration",
            Self::InvalidSourcePath => "invalid-source-path",
            Self::SourcePathIo => "source-path-io",
            Self::SourcePathWrongType => "source-path-wrong-type",
            Self::SourcePathOutsideBoundary => "source-path-outside-boundary",
            Self::InvalidRepositoryReference => "invalid-repository-reference",
            Self::DuplicateRepositoryId => "duplicate-repository-id",
            Self::DuplicatePackageId => "duplicate-package-id",
            Self::DuplicateTargetId => "duplicate-target-id",
            Self::DuplicateContentId => "duplicate-content-id",
            Self::DuplicateConceptId => "duplicate-concept-id",
            Self::DuplicatePackageSlug => "duplicate-package-slug",
            Self::UnknownContentOwner => "unknown-content-owner",
            Self::UnknownConceptPackage => "unknown-concept-package",
            Self::UnknownRelationshipEndpoint => "unknown-relationship-endpoint",
            Self::InvalidExternalPackageCoordinate => "invalid-external-package-coordinate",
            Self::PythonMetadata => "python-metadata",
            Self::PythonDynamicMetadata => "python-dynamic-metadata",
            Self::PythonSyntax => "python-syntax",
            Self::PythonUnsupportedVersion => "python-unsupported-version",
            Self::PythonUnsupportedSyntax => "python-unsupported-syntax",
            Self::PythonSourceRead => "python-source-read",
            Self::PythonModuleCollision => "python-module-collision",
            Self::PythonDynamicExport => "python-dynamic-export",
            Self::PythonUnresolvedReexport => "python-unresolved-reexport",
            Self::PythonConflictingStub => "python-conflicting-stub",
            Self::PythonUnsupportedSurface => "python-unsupported-surface",
            Self::PythonDuplicateIdentity => "python-duplicate-identity",
            Self::PythonConflictingAlias => "python-conflicting-alias",
            Self::PythonInvalidIdentity => "python-invalid-identity",
            Self::PythonIncompleteDocstring => "python-incomplete-docstring",
            Self::PythonUnsupportedDocstring => "python-unsupported-docstring",
            Self::PythonDocstringSourceAttribution => "python-docstring-source-attribution",
        }
    }
}

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// The input is invalid. Errors sort before warnings.
    Error,
    /// Processing can continue, but the condition is visible.
    Warning,
}

/// A normalized, nonempty relative path using forward-slash separators.
///
/// Construction and deserialization reject absolute paths, colons (including
/// Windows prefixes), backslashes, NUL bytes, and empty, `.` or `..` components.
/// This type performs no filesystem access or symlink validation. Callers supply
/// paths relative to the source's declared root, never canonical machine paths.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DiagnosticPath(#[serde(deserialize_with = "deserialize_path")] String);

/// A diagnostic path is not a normalized portable relative path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error(
    "diagnostic paths must be nonempty normalized relative paths with forward-slash separators"
)]
pub struct InvalidDiagnosticPath;

impl DiagnosticPath {
    /// The normalized relative spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DiagnosticPath {
    type Error = InvalidDiagnosticPath;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.contains(['\\', '\0', ':'])
            || value
                .split('/')
                .any(|component| matches!(component, "" | "." | ".."))
        {
            return Err(InvalidDiagnosticPath);
        }
        Ok(Self(value))
    }
}

impl TryFrom<&str> for DiagnosticPath {
    type Error = InvalidDiagnosticPath;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_owned())
    }
}

fn deserialize_path<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let path = DiagnosticPath::try_from(String::deserialize(deserializer)?)
        .map_err(serde::de::Error::custom)?;
    Ok(path.0)
}

/// Portable origin of the primary and related spans in a diagnostic.
///
/// Configuration sources sort before repository sources. Configuration paths
/// sort lexically; repository sources sort by repository ID, then path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DiagnosticSource {
    /// A file relative to the workspace configuration directory.
    Configuration {
        /// Configuration-relative path, usually `diplodocus.toml`.
        path: DiagnosticPath,
    },
    /// A file within an explicitly declared repository.
    Repository {
        /// Stable repository ID, independent of its checkout directory.
        repository: String,
        /// Repository-relative source path.
        path: DiagnosticPath,
    },
}

/// Semantic entity related to a diagnostic, independent of rendered URLs.
///
/// Variants sort by the `kind` spelling below, then fields in declaration order.
/// Indices are zero-based declaration positions and sort numerically. These
/// references retain declared identities without checking that they resolve.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DiagnosticEntity {
    /// A conceptual API group.
    Concept {
        /// Declared concept ID.
        id: String,
    },
    /// The workspace configuration as a whole.
    Configuration,
    /// An indexed configuration field, before a semantic entity is available.
    ConfigurationField {
        /// Schema field path, such as `package[0].slug`.
        path: String,
    },
    /// An authored content collection.
    Content {
        /// Declared collection ID.
        id: String,
    },
    /// An authored document in a content collection.
    Document {
        /// Declared collection ID.
        collection: String,
        /// Collection-relative document path.
        path: DiagnosticPath,
    },
    /// An extracted item scoped to a package.
    Item {
        /// Declared package ID.
        package: String,
        /// Extractor-assigned item ID.
        id: String,
    },
    /// A package or internal component.
    Package {
        /// Declared package ID.
        id: String,
    },
    /// The workspace project.
    Project,
    /// A relationship declaration, which has no authored ID.
    Relationship {
        /// Index in the workspace's relationship declarations.
        index: usize,
    },
    /// An explicitly declared repository.
    Repository {
        /// Declared repository ID.
        id: String,
    },
    /// An extraction target scoped to its package.
    Target {
        /// Declared package ID.
        package: String,
        /// Declared target ID.
        id: String,
    },
}

/// A diagnostic with optional portable entity and source context.
///
/// Sorting uses source, primary span `(start, end)`, stable code spelling,
/// severity (error before warning), related entity, message, and finally related
/// spans lexicographically by `(start, end)`. Missing sources, primary spans,
/// and entities sort after present values. Every field participates, so equal
/// sort keys have identical serialized representations regardless of input order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Stable identifier.
    pub code: DiagnosticCode,
    /// Severity.
    pub severity: Severity,
    /// Human-readable explanation.
    /// Producers must not embed runtime paths or other machine-specific data.
    pub message: String,
    /// Related semantic entity or configuration field when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_entity: Option<DiagnosticEntity>,
    /// Portable source path and its root when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<DiagnosticSource>,
    /// Source range when available.
    pub span: Option<SourceSpan>,
    /// Additional declarations contributing to this diagnostic, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_spans: Vec<SourceSpan>,
}

impl Diagnostic {
    /// Create a diagnostic without inventing unavailable source or entity context.
    pub fn new(code: DiagnosticCode, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            code,
            severity,
            message: message.into(),
            related_entity: None,
            source: None,
            span: None,
            related_spans: Vec::new(),
        }
    }

    /// Attach the portable source shared by the primary and related spans.
    pub fn with_source(mut self, source: DiagnosticSource) -> Self {
        self.source = Some(source);
        self
    }

    /// Attach a semantic entity or configuration field.
    pub fn with_entity(mut self, entity: DiagnosticEntity) -> Self {
        self.related_entity = Some(entity);
        self
    }
}

impl Ord for Diagnostic {
    fn cmp(&self, other: &Self) -> Ordering {
        optional_last(&self.source, &other.source)
            .then_with(|| optional_last(&self.span.map(span_key), &other.span.map(span_key)))
            .then_with(|| self.code.as_str().cmp(other.code.as_str()))
            .then_with(|| self.severity.cmp(&other.severity))
            .then_with(|| optional_last(&self.related_entity, &other.related_entity))
            .then_with(|| self.message.cmp(&other.message))
            .then_with(|| {
                self.related_spans
                    .iter()
                    .copied()
                    .map(span_key)
                    .cmp(other.related_spans.iter().copied().map(span_key))
            })
    }
}

impl PartialOrd for Diagnostic {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn span_key(span: SourceSpan) -> (usize, usize) {
    (span.start, span.end)
}

fn optional_last<T: Ord>(left: &Option<T>, right: &Option<T>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}
