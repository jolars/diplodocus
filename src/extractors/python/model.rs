//! Source observations shared by Python parsing and semantic reconciliation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, DiagnosticPath};
use crate::ir::{
    Fingerprint, Provenance, SignatureExpression, SourceLocation, SourceSpan, SourcedSignature,
    TargetReference,
};

/// Parsed inputs for one explicitly configured target, before item identity or
/// visibility is assigned. Invalid modules retain their input, not recovered facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedPythonPackage {
    /// Stable workspace package ID.
    pub package: String,
    /// Explicit target identity.
    pub target: TargetReference,
    /// Static metadata, absent when required fields are invalid or dynamic.
    pub metadata: Option<PythonMetadata>,
    /// Source and stub variants, ordered by portable input path.
    pub modules: Vec<ParsedModule>,
    /// Exact contributing inputs, including metadata and typing markers.
    pub inputs: BTreeMap<DiagnosticPath, PythonInput>,
    /// Every observed failure, sorted by the shared diagnostic ordering.
    pub diagnostics: Vec<Diagnostic>,
    /// Parser observations and input hashes for this static source pass.
    /// Later semantic passes extend capability and parser claims when they run.
    pub provenance: Provenance,
}

/// Static PEP 621 metadata; dependencies use normalized PEP 508 spelling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PythonMetadata {
    /// Distribution name, independent of its configured documentation ID.
    pub name: String,
    /// PEP 440 distribution version.
    pub version: String,
    /// Optional short description.
    pub description: Option<String>,
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

/// A file read by the source adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PythonInput {
    /// Role of this file.
    pub kind: PythonInputKind,
    /// Portable, file-level location.
    pub source: SourceLocation,
    /// Original UTF-8 source retained without evaluating it. Empty when
    /// decoding failed; in that case `raw_bytes` retains the original input.
    pub text: String,
    /// Undecodable original bytes, never parsed as replacement characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_bytes: Option<Vec<u8>>,
    /// Hash of the original bytes.
    pub fingerprint: Fingerprint,
}

/// Input classification, independent of filename conventions in consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PythonInputKind {
    /// PEP 621 metadata.
    Metadata,
    /// Python implementation.
    Source,
    /// Maintained Python stub, including native declarations.
    Stub,
    /// PEP 561 typing marker.
    TypedMarker,
}

/// Ruff grammar mode used for a module variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PythonSourceKind {
    /// Implementation source.
    Source,
    /// Maintained stub source.
    Stub,
}

/// One source or stub variant of a canonical importable module name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedModule {
    /// Canonical dotted module name derived from the configured module root.
    pub name: String,
    /// Whether this file is an `__init__` module.
    pub is_package: bool,
    /// Implementation or stub grammar.
    pub kind: PythonSourceKind,
    /// File-level source evidence.
    pub source: SourceLocation,
    /// Decoded module prose with independently proven source mappings.
    pub docstring: Option<ParsedDocstring>,
    /// Definitions in source order, preserving duplicates for reconciliation.
    pub declarations: Vec<ParsedDeclaration>,
    /// Module-scope imports in source order.
    pub imports: Vec<ParsedImport>,
    /// Writes to `__all__` in source order, including unsupported writes.
    pub exports: Vec<ExportOperation>,
    /// False if malformed or version-incompatible syntax prevents authority.
    pub valid: bool,
}

/// A lexical declaration before stub merging or decorator interpretation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedDeclaration {
    /// Unqualified binding name.
    pub name: String,
    /// Source declaration form.
    pub kind: DeclarationKind,
    /// Full declaration evidence.
    pub source: SourceLocation,
    /// Structured callable or value signature with its own evidence.
    pub signature: Option<SourcedSignature>,
    /// Unevaluated decorator expressions in source order.
    pub decorators: Vec<ParsedDecorator>,
    /// Whether the function uses `async def`.
    pub is_async: bool,
    /// Base expressions of a class, in source order.
    pub bases: Vec<SignatureExpression>,
    /// Class members, including methods, nested classes, and attributes.
    pub members: Vec<ParsedDeclaration>,
    /// Function/class prose or the immediately following attribute docstring.
    pub docstring: Option<ParsedDocstring>,
}

/// Lexical forms whose semantic role is assigned by reconciliation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeclarationKind {
    /// Includes ordinary functions, methods, overloads, and property accessors.
    Function,
    /// A class definition.
    Class,
    /// Includes annotated fields, constants, and possible assignment aliases.
    Assignment,
    /// An explicit `type` declaration.
    TypeAlias,
}

/// Decorator syntax without a claim about runtime behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedDecorator {
    /// Structured unevaluated expression.
    pub expression: SignatureExpression,
    /// Exact decorator source location.
    pub source: SourceLocation,
}

/// A module-scope import; a missing module and zero level is an ordinary import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedImport {
    /// Module of a `from` import, absent for `from . import` and ordinary imports.
    pub module: Option<String>,
    /// Number of leading relative-import dots; zero means absolute.
    pub level: u32,
    /// Imported spellings, including `*`, in source order.
    pub names: Vec<ImportedName>,
    /// Full import statement location.
    pub source: SourceLocation,
}

/// A single imported binding before name resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedName {
    /// Imported dotted name or `*`.
    pub name: String,
    /// Explicit local alias, including a redundant alias.
    pub alias: Option<String>,
}

/// An export operation preserved for a source-ordered static evaluation pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportOperation {
    /// Assignment, supported mutation, or an unrecognized operation.
    pub kind: ExportOperationKind,
    /// Literal, name, concatenation, or preserved unsupported expression.
    pub value: ExportValue,
    /// Operation source, never an inferred evaluation location.
    pub source: SourceLocation,
}

/// Operations in the supported static export subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExportOperationKind {
    /// Replace the export list.
    Assign,
    /// Extend with a list, tuple, or concatenation (`+=` or `extend`).
    Extend,
    /// Append one literal name.
    Append,
    /// Dynamic or unsupported mutation, retained visibly.
    Unsupported,
}

/// Expression syntax considered by the static export evaluator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum ExportValue {
    /// Literal strings, in declaration order.
    Names(Vec<String>),
    /// A referenced binding, requiring a statically proven value.
    Name(String),
    /// Ordered operands of list or tuple concatenation.
    Concat(Vec<ExportValue>),
    /// Original expression outside the supported subset.
    Unsupported(String),
}

/// A decoded docstring and the original evidence used to locate its prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedDocstring {
    /// Ruff-decoded text, without indentation cleanup or other normalization.
    pub text: String,
    /// Enclosing original literal expression; fallback for unmappable regions.
    pub source: SourceLocation,
    /// Exact equal-length, identical-byte mappings in decoded order. Gaps mean
    /// unknown mappings, never permission to add decoded offsets to raw offsets.
    pub segments: Vec<DocstringSourceSegment>,
}

/// A proven byte-for-byte interval in decoded and original source text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocstringSourceSegment {
    /// Half-open UTF-8 range in the decoded docstring.
    pub decoded: SourceSpan,
    /// Half-open UTF-8 range in its original source file.
    pub source: SourceSpan,
}
