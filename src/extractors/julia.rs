//! Static Julia extraction from Project.toml and an explicit source entry file.
//!
//! Fatou owns syntax and string decoding. This adapter owns filesystem boundaries,
//! API policy, identities, and portable evidence; it never starts Julia.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticPath, DiagnosticSource, Severity};
use crate::ir::*;
use crate::paths::{
    ResolvedPackagePaths, ResolvedRepositoryPaths, ResolvedTargetPath, resolve_input_file,
};
use crate::provenance::{BuiltinExtractor, ExtractionObservation, fingerprint_bytes};

pub mod docstrings;
mod metadata;
mod signatures;
mod source;
mod surface;

pub use metadata::{JuliaMetadata, JuliaMetadataField};

pub(crate) const FATOU_VERSION: &str = "0.8.1";
pub(crate) const CAPABILITIES: &[&str] = &[
    "diagnostics.unsupported-visible",
    "julia.declarations",
    "julia.docs.markdown",
    "julia.docs.references",
    "julia.exports.static",
    "julia.includes.static",
    "julia.metadata.project",
    "julia.methods",
    "julia.reexports",
    "provenance.source",
];

/// Portable Julia extraction ready for workspace assembly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JuliaExtraction {
    /// Shared portable vocabulary version.
    pub schema_version: SchemaVersion,
    /// Configured workspace package ID.
    pub package: String,
    /// Explicit extraction target.
    pub target: TargetReference,
    /// Validated metadata, absent when required values are malformed.
    pub metadata: Option<JuliaMetadata>,
    /// Canonical API items, with addressable authored methods.
    pub items: BTreeMap<String, Item>,
    /// Every consumed input, including malformed original bytes.
    pub inputs: BTreeMap<DiagnosticPath, JuliaInput>,
    /// Diagnostics in deterministic order.
    pub diagnostics: Vec<Diagnostic>,
    /// Static parser observations and input fingerprints.
    pub provenance: Provenance,
}

/// Original input retained independently from semantic extraction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JuliaInput {
    /// Either `package-metadata` or `julia-source`.
    pub kind: String,
    /// Repository-relative input location.
    pub source: SourceLocation,
    /// Original source when it is UTF-8.
    pub text: Option<String>,
    /// Original bytes when UTF-8 decoding failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_bytes: Option<Vec<u8>>,
}

/// Extract a maintained Julia entry file and its unconditional literal includes.
///
/// Files may be included from anywhere inside the configured package, but reads
/// never escape that package or repository. Error-bearing results are inspectable
/// and must be rejected before publication. No depot or runtime is consulted.
pub fn extract_target(
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
    target: &ResolvedTargetPath,
) -> JuliaExtraction {
    let reference = TargetReference {
        package: package.id.clone(),
        target: target.id.clone(),
    };
    let mut observation = ExtractionObservation {
        target: reference.clone(),
        extractor: BuiltinExtractor::Julia,
        capabilities: CAPABILITIES.iter().map(|s| (*s).into()).collect(),
        parsers: BTreeMap::new(),
        inputs: BTreeMap::new(),
    };
    let mut result = JuliaExtraction {
        schema_version: SchemaVersion,
        package: package.id.clone(),
        target: reference,
        metadata: None,
        items: BTreeMap::new(),
        inputs: BTreeMap::new(),
        diagnostics: Vec::new(),
        provenance: observation.clone().into_provenance(None, None),
    };
    if !package.path.starts_with(&repository.path)
        || fs::canonicalize(&package.path).ok().as_ref() != Some(&package.path)
        || !target.path.starts_with(&package.path)
        || !target.path.is_file()
        || target
            .path
            .extension()
            .is_none_or(|extension| extension != "jl")
    {
        result.diagnostics.push(Diagnostic::new(
            DiagnosticCode::JuliaSourceRead,
            Severity::Error,
            "Julia targets must be explicit .jl entry files inside the configured package.",
        ));
        return result;
    }
    if let Some(input) = read_input(
        repository,
        package,
        &package.metadata_path,
        "package-metadata",
        &mut result,
        &mut observation,
    ) && let Some(text) = &input.text
    {
        result.metadata = metadata::parse(text, &input.source, &mut result.diagnostics);
    }
    let declarations = source::extract(
        repository,
        package,
        &target.path,
        &mut result,
        &mut observation,
    );
    surface::assemble(declarations, &mut result);
    result.provenance = observation.into_provenance(None, None);
    result.diagnostics.sort();
    result.diagnostics.dedup();
    result
}

fn read_input(
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
    path: &Path,
    kind: &str,
    result: &mut JuliaExtraction,
    observation: &mut ExtractionObservation,
) -> Option<JuliaInput> {
    let declared = path.strip_prefix(&package.path).ok()?;
    let resolved = match resolve_input_file(&package.path, "julia.input", declared, &package.path) {
        Ok(path) => path,
        Err(error) => {
            let mut diagnostic = error.to_diagnostic("julia-input".try_into().unwrap());
            diagnostic.source = None;
            diagnostic.related_entity = None;
            result.diagnostics.push(diagnostic);
            return None;
        }
    };
    if crate::generated_storage::is_generated_file(&resolved) {
        result.diagnostics.push(Diagnostic::new(
            DiagnosticCode::JuliaSourceRead,
            Severity::Error,
            "Generated storage cannot be a Julia source input.",
        ));
        return None;
    }
    let location = repository.source_location(&resolved, None).ok()?;
    if let Some(input) = result.inputs.get(&location.path) {
        return Some(input.clone());
    }
    let bytes = match fs::read(&resolved) {
        Ok(bytes) => bytes,
        Err(error) => {
            result.diagnostics.push(diagnostic(
                DiagnosticCode::JuliaSourceRead,
                Severity::Error,
                format!("Cannot read Julia input: {:?}.", error.kind()),
                &location,
            ));
            return None;
        }
    };
    let fingerprint = fingerprint_bytes(&bytes);
    let (text, raw_bytes) = match String::from_utf8(bytes) {
        Ok(text) if text.len() <= u32::MAX as usize => (Some(text), None),
        Ok(_) => {
            result.diagnostics.push(diagnostic(
                DiagnosticCode::JuliaSourceRead,
                Severity::Error,
                "Julia input exceeds Fatou's byte-range limit.",
                &location,
            ));
            return None;
        }
        Err(error) => {
            result.diagnostics.push(diagnostic(
                DiagnosticCode::JuliaSourceRead,
                Severity::Error,
                "Julia inputs must use UTF-8 encoding.",
                &location,
            ));
            (None, Some(error.into_bytes()))
        }
    };
    let parsers = if text.is_some() {
        if kind == "package-metadata" {
            observation
                .parsers
                .entry("toml".into())
                .or_insert(ParserProvenance {
                    version: "1.1.6".into(),
                    role: "julia-metadata".into(),
                    settings: BTreeMap::new(),
                });
            BTreeSet::from(["toml".into()])
        } else {
            observation
                .parsers
                .entry("fatou-parser".into())
                .or_insert(ParserProvenance {
                    version: FATOU_VERSION.into(),
                    role: "julia-source-and-documentation".into(),
                    settings: BTreeMap::from([
                        ("grammar".into(), "julia-superset".into()),
                        ("includes".into(), "unconditional-literal".into()),
                        ("surface".into(), "public-or-documented".into()),
                        ("documentation".into(), "julia-markdown-inert".into()),
                    ]),
                });
            observation
                .parsers
                .entry("rowan".into())
                .or_insert(ParserProvenance {
                    version: "0.17.0".into(),
                    role: "syntax-tree-and-byte-ranges".into(),
                    settings: BTreeMap::new(),
                });
            BTreeSet::from(["fatou-parser".into(), "rowan".into()])
        }
    } else {
        BTreeSet::new()
    };
    observation
        .inputs
        .entry(location.repository.clone())
        .or_default()
        .insert(
            location.path.clone(),
            ExtractionInput {
                kind: kind.into(),
                fingerprint,
                parsers,
            },
        );
    let input = JuliaInput {
        kind: kind.into(),
        source: location,
        text,
        raw_bytes,
    };
    result
        .inputs
        .insert(input.source.path.clone(), input.clone());
    Some(input)
}

pub(crate) fn diagnostic(
    code: DiagnosticCode,
    severity: Severity,
    message: impl Into<String>,
    source: &SourceLocation,
) -> Diagnostic {
    let mut diagnostic =
        Diagnostic::new(code, severity, message).with_source(DiagnosticSource::Repository {
            repository: source.repository.clone(),
            path: source.path.clone(),
        });
    diagnostic.span = source.span;
    diagnostic
}

pub(crate) fn located(source: &SourceLocation, range: rowan::TextRange) -> SourceLocation {
    SourceLocation {
        span: Some(span(range)),
        ..source.clone()
    }
}

pub(crate) fn span(range: rowan::TextRange) -> SourceSpan {
    SourceSpan {
        start: u32::from(range.start()) as usize,
        end: u32::from(range.end()) as usize,
    }
}

pub(crate) fn evidence(source: &SourceLocation, role: SourceRole) -> SourceEvidence {
    SourceEvidence {
        source: source.clone(),
        role,
        parsers: BTreeSet::from(["fatou-parser".into(), "rowan".into()]),
    }
}

pub(crate) fn declaration_provenance(source: &SourceLocation) -> Provenance {
    Provenance {
        activity: ProvenanceActivity::Declaration,
        source: Some(DiagnosticSource::Repository {
            repository: source.repository.clone(),
            path: source.path.clone(),
        }),
        span: source.span,
        tools: BTreeMap::from([
            ("fatou-parser".into(), FATOU_VERSION.into()),
            ("rowan".into(), "0.17.0".into()),
            ("julia".into(), env!("CARGO_PKG_VERSION").into()),
        ]),
    }
}

pub(crate) fn method_selector(text: &str) -> Option<(String, SignatureExpression)> {
    signatures::selector(text)
}

fn normalized_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}
