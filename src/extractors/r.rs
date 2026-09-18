//! Static R extraction from maintained DESCRIPTION, NAMESPACE, R, and Rd files.
//!
//! The adapter never starts a runtime or evaluates package code. Native syntax
//! stays internal; results contain only portable Diplodocus records.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticPath, DiagnosticSource, Severity};
use crate::ir::{
    ExtractionInput, Item, ParserProvenance, Provenance, ProvenanceActivity, SchemaVersion,
    SourceEvidence, SourceLocation, SourceRole, SourceSpan, TargetReference,
};
use crate::paths::{
    PathResolutionError, ResolvedPackagePaths, ResolvedRepositoryPaths, ResolvedTargetPath,
    resolve_input_file,
};
use crate::provenance::{BuiltinExtractor, ExtractionObservation, fingerprint_bytes};

mod metadata;
mod namespace;
mod rd;
mod source;
mod surface;

pub use metadata::{RDependency, RMetadata, RMetadataField, RVersionConstraint};

/// Portable extraction result ready for later workspace merging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RExtraction {
    /// Shared portable vocabulary version.
    pub schema_version: SchemaVersion,
    /// Workspace package ID.
    pub package: String,
    /// Explicit configured target.
    pub target: TargetReference,
    /// Validated metadata, absent on malformed input.
    pub metadata: Option<RMetadata>,
    /// Canonical items with independent signatures and documentation.
    pub items: BTreeMap<String, Item>,
    /// Every consumed input, including malformed bytes retained for inspection.
    pub inputs: BTreeMap<DiagnosticPath, RInput>,
    /// Diagnostics in the shared deterministic order.
    pub diagnostics: Vec<Diagnostic>,
    /// Actual parser observations and content fingerprints.
    pub provenance: Provenance,
}

/// Original input retained independently of successful semantic extraction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RInput {
    /// Stable input kind: package-metadata, namespace, r-source, or rd.
    pub kind: String,
    /// File-level portable source.
    pub source: SourceLocation,
    /// Original UTF-8 input, absent when decoding failed.
    pub text: Option<String>,
    /// Undecodable original bytes, absent for UTF-8 input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_bytes: Option<Vec<u8>>,
}

/// Extract one R package-layout directory without installing or loading it.
///
/// The target contains NAMESPACE, R/, and man/; metadata uses the configured
/// package metadata path. Missing R/ or man/ directories are allowed. Reads stay
/// inside the package and target boundaries, except the explicitly configured
/// metadata input. Directory symlinks are rejected. Error-bearing results are
/// inspectable, but callers must reject them before building a site.
pub fn extract_target(
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
    target: &ResolvedTargetPath,
) -> RExtraction {
    let reference = TargetReference {
        package: package.id.clone(),
        target: target.id.clone(),
    };
    let observation = ExtractionObservation {
        target: reference.clone(),
        extractor: BuiltinExtractor::R,
        capabilities: BTreeSet::new(),
        parsers: BTreeMap::new(),
        inputs: BTreeMap::new(),
    };
    let mut result = RExtraction {
        schema_version: SchemaVersion,
        package: package.id.clone(),
        target: reference,
        metadata: None,
        items: BTreeMap::new(),
        inputs: BTreeMap::new(),
        diagnostics: vec![],
        provenance: observation.into_provenance(None, None),
    };
    if !package.path.starts_with(&repository.path)
        || !target.path.starts_with(&package.path)
        || fs::canonicalize(&package.path).ok().as_ref() != Some(&package.path)
        || fs::canonicalize(&target.path).ok().as_ref() != Some(&target.path)
        || !target.path.is_dir()
    {
        result.diagnostics.push(Diagnostic::new(
            DiagnosticCode::SourcePathOutsideBoundary,
            Severity::Error,
            "R package and target must be canonical directories inside the configured repository.",
        ));
        return result;
    }
    read_input(
        repository,
        &package.path,
        &package.metadata_path,
        "package-metadata",
        &mut result,
    );
    read_input(
        repository,
        &target.path,
        &target.path.join("NAMESPACE"),
        "namespace",
        &mut result,
    );
    for (directory, extension, kind) in [("R", "R", "r-source"), ("man", "Rd", "rd")] {
        discover(
            repository,
            &target.path,
            &target.path.join(directory),
            extension,
            kind,
            &mut result,
        );
    }
    let mut definitions = vec![];
    let mut namespace = namespace::Namespace::default();
    let mut topics = vec![];
    for input in result.inputs.values() {
        let Some(text) = &input.text else { continue };
        match input.kind.as_str() {
            "package-metadata" => {
                result.metadata = metadata::parse(text, &input.source, &mut result.diagnostics)
            }
            "namespace" => {
                namespace = namespace::parse(text, &input.source, &mut result.diagnostics)
            }
            "r-source" => {
                definitions.extend(source::parse(text, &input.source, &mut result.diagnostics))
            }
            "rd" => {
                if let Some(topic) = rd::parse(text, &input.source, &mut result.diagnostics) {
                    if let ProvenanceActivity::Extraction { inputs, .. } =
                        &mut result.provenance.activity
                    {
                        let record = inputs
                            .get_mut(&input.source.repository)
                            .unwrap()
                            .get_mut(&input.source.path)
                            .unwrap();
                        record.parsers.insert("rd-ast".into());
                        if topic.uses_r {
                            record.parsers.insert("arity-parser".into());
                        }
                    }
                    topics.push(topic);
                }
            }
            _ => unreachable!(),
        }
    }
    source::reject_shadowed_builtins(&mut definitions, namespace.imports.keys().cloned());
    result.items = surface::reconcile(
        &package.id,
        &definitions,
        &namespace,
        &mut result.diagnostics,
    );
    rd::attach(&mut result.items, &topics, &mut result.diagnostics);
    result.diagnostics.sort();
    result.diagnostics.dedup();
    finish_provenance(&mut result);
    result
}

fn read_input(
    repository: &ResolvedRepositoryPaths,
    boundary: &Path,
    path: &Path,
    kind: &str,
    result: &mut RExtraction,
) {
    let Some(relative) = path.strip_prefix(boundary).ok() else {
        result.diagnostics.push(Diagnostic::new(
            DiagnosticCode::SourcePathOutsideBoundary,
            Severity::Error,
            "R input escapes its declared boundary.",
        ));
        return;
    };
    let canonical = match resolve_input_file(boundary, "r.input", relative, boundary) {
        Ok(path) => path,
        Err(error) => {
            result
                .diagnostics
                .push(path_diagnostic(repository, path, error));
            return;
        }
    };
    let source = match repository.source_location(&canonical, None) {
        Ok(source) => source,
        Err(error) => {
            result
                .diagnostics
                .push(path_diagnostic(repository, &canonical, error));
            return;
        }
    };
    let bytes = match fs::read(&canonical) {
        Ok(bytes) => bytes,
        Err(error) => {
            result.diagnostics.push(diagnostic(
                DiagnosticCode::RSourceRead,
                Severity::Error,
                format!("Cannot read R input: {:?}.", error.kind()),
                &source,
            ));
            return;
        }
    };
    let fingerprint = fingerprint_bytes(&bytes);
    let (text, raw_bytes) = match String::from_utf8(bytes) {
        Ok(text) if text.len() <= u32::MAX as usize => (Some(text), None),
        Ok(_) => {
            result.diagnostics.push(diagnostic(
                DiagnosticCode::RSourceRead,
                Severity::Error,
                "R input exceeds the parser's byte-range limit.",
                &source,
            ));
            return;
        }
        Err(error) => {
            result.diagnostics.push(diagnostic(
                if kind == "rd" {
                    DiagnosticCode::RRdSyntax
                } else {
                    DiagnosticCode::RSourceRead
                },
                Severity::Error,
                "R inputs must use UTF-8 encoding.",
                &source,
            ));
            (None, Some(error.into_bytes()))
        }
    };
    let parsers = if text.is_none() {
        BTreeSet::new()
    } else if kind == "rd" {
        BTreeSet::from(["rd-source".into()])
    } else {
        BTreeSet::from(["arity-parser".into()])
    };
    if let ProvenanceActivity::Extraction { inputs, .. } = &mut result.provenance.activity {
        inputs.entry(repository.id.clone()).or_default().insert(
            source.path.clone(),
            ExtractionInput {
                kind: kind.into(),
                fingerprint,
                parsers,
            },
        );
    }
    result.inputs.insert(
        source.path.clone(),
        RInput {
            kind: kind.into(),
            source,
            text,
            raw_bytes,
        },
    );
}

fn path_diagnostic(
    repository: &ResolvedRepositoryPaths,
    path: &Path,
    error: PathResolutionError,
) -> Diagnostic {
    let mut d = error.to_diagnostic("r-input".try_into().unwrap());
    d.source = path
        .strip_prefix(&repository.path)
        .ok()
        .and_then(|p| p.to_str())
        .and_then(|p| DiagnosticPath::try_from(p.replace('\\', "/")).ok())
        .map(|path| DiagnosticSource::Repository {
            repository: repository.id.clone(),
            path,
        });
    d.related_entity = None;
    d
}

fn discover(
    repository: &ResolvedRepositoryPaths,
    boundary: &Path,
    path: &Path,
    extension: &str,
    kind: &str,
    result: &mut RExtraction,
) {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            result.diagnostics.push(Diagnostic::new(
                DiagnosticCode::RSourceRead,
                Severity::Error,
                "Cannot inspect an R input directory.",
            ));
            return;
        }
    };
    if metadata.is_symlink() {
        if fs::metadata(path).is_ok_and(|m| m.is_file())
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case(extension))
        {
            read_input(repository, boundary, path, kind, result);
        } else {
            result.diagnostics.push(Diagnostic::new(
                DiagnosticCode::RSourceRead,
                Severity::Error,
                "Directory and broken symlinks in R targets are unsupported.",
            ));
        }
    } else if metadata.is_file() {
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case(extension))
        {
            read_input(repository, boundary, path, kind, result);
        }
    } else if metadata.is_dir() {
        let entries =
            match fs::read_dir(path).and_then(|entries| entries.collect::<Result<Vec<_>, _>>()) {
                Ok(mut entries) => {
                    entries.sort_by_key(|entry| entry.file_name());
                    entries
                }
                Err(_) => {
                    result.diagnostics.push(Diagnostic::new(
                        DiagnosticCode::RSourceRead,
                        Severity::Error,
                        "Cannot list an R input directory.",
                    ));
                    return;
                }
            };
        for entry in entries {
            if !entry
                .file_name()
                .to_str()
                .is_some_and(|n| n.starts_with('.'))
            {
                discover(repository, boundary, &entry.path(), extension, kind, result);
            }
        }
    }
}

fn finish_provenance(result: &mut RExtraction) {
    let ProvenanceActivity::Extraction {
        capabilities,
        parsers,
        inputs,
        ..
    } = &mut result.provenance.activity
    else {
        unreachable!()
    };
    capabilities.extend(
        [
            "diagnostics.unsupported-visible",
            "provenance.source",
            "r.docs.rd",
            "r.metadata.dcf",
            "r.namespace.static",
            "r.s3",
            "r.source.functions",
        ]
        .map(str::to_owned),
    );
    let mut grammars = BTreeMap::new();
    for input in result.inputs.values().filter(|input| input.text.is_some()) {
        if inputs[&input.source.repository][&input.source.path]
            .parsers
            .contains("arity-parser")
        {
            grammars.insert(
                format!("grammar:{}", input.source.path.as_str()),
                match input.kind.as_str() {
                    "package-metadata" => "dcf",
                    "namespace" => "namespace",
                    _ => "r",
                }
                .into(),
            );
        }
    }
    if !grammars.is_empty() {
        parsers.insert(
            "arity-parser".into(),
            ParserProvenance {
                version: "0.6.0".into(),
                role: "r-metadata-namespace-source".into(),
                settings: grammars,
            },
        );
    }
    if result
        .inputs
        .values()
        .any(|input| input.kind == "rd" && input.text.is_some())
    {
        for (name, role, settings) in [
            (
                "rd-source",
                "rd-syntax",
                BTreeMap::from([("dynamic_markup".into(), "unresolved".into())]),
            ),
            (
                "rd-ast",
                "rd-semantics",
                BTreeMap::from([
                    ("views".into(), "strict".into()),
                    ("source_locations".into(), "file".into()),
                    ("default_features".into(), "false".into()),
                ]),
            ),
        ] {
            if !inputs
                .values()
                .flat_map(|files| files.values())
                .any(|input| input.parsers.contains(name))
            {
                continue;
            }
            parsers.insert(
                name.into(),
                ParserProvenance {
                    version: "0.4.0".into(),
                    role: role.into(),
                    settings,
                },
            );
        }
    }
    result.provenance.tools.extend(
        parsers
            .iter()
            .map(|(name, parser)| (name.clone(), parser.version.clone())),
    );
}

fn diagnostic(
    code: DiagnosticCode,
    severity: Severity,
    message: impl Into<String>,
    source: &SourceLocation,
) -> Diagnostic {
    let mut d =
        Diagnostic::new(code, severity, message).with_source(DiagnosticSource::Repository {
            repository: source.repository.clone(),
            path: source.path.clone(),
        });
    d.span = source.span;
    d
}

fn located(source: &SourceLocation, start: usize, end: usize) -> SourceLocation {
    SourceLocation {
        span: Some(SourceSpan { start, end }),
        ..source.clone()
    }
}

fn evidence(source: &SourceLocation, role: SourceRole) -> SourceEvidence {
    SourceEvidence {
        source: source.clone(),
        role,
        parsers: BTreeSet::from(["arity-parser".into()]),
    }
}

fn provenance(source: &SourceLocation, rd: bool) -> Provenance {
    let mut tools = BTreeMap::from([
        ("diplodocus".into(), env!("CARGO_PKG_VERSION").into()),
        ("r".into(), env!("CARGO_PKG_VERSION").into()),
    ]);
    if rd {
        tools.extend([
            ("rd-source".into(), "0.4.0".into()),
            ("rd-ast".into(), "0.4.0".into()),
        ]);
    } else {
        tools.insert("arity-parser".into(), "0.6.0".into());
    }
    Provenance {
        activity: ProvenanceActivity::Declaration,
        source: Some(DiagnosticSource::Repository {
            repository: source.repository.clone(),
            path: source.path.clone(),
        }),
        span: source.span,
        tools,
    }
}
