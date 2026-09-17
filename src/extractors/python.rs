//! Static Python source observations, independent of public-surface reconciliation.
//!
//! The adapter reads a configured module file or package directory, metadata,
//! source/stub variants, and typing markers. It never starts a process, imports
//! Python, or invokes a build backend. Native parser types remain internal.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use ruff_text_size::TextRange;

use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticSource, Severity};
use crate::ir::{ExtractionInput, ParserProvenance, SourceLocation, SourceSpan, TargetReference};
use crate::paths::{
    PathResolutionError, ResolvedPackagePaths, ResolvedRepositoryPaths, ResolvedTargetPath,
    resolve_input_file,
};
use crate::provenance::{BuiltinExtractor, ExtractionObservation, fingerprint_bytes};

mod metadata;
mod model;
mod signatures;
mod source;

pub use model::*;

/// Parse one explicitly selected Python module file or package directory.
///
/// A directory is the importable package or namespace-package root: its final
/// component is the root module name. A file's enclosing `__init__` directories
/// establish its qualified name within the configured package boundary. Inputs
/// and variants are sorted by portable path, with exact content fingerprints.
/// Errors are returned as portable diagnostics; independent files are inspected
/// even when one file is malformed. No recovered AST contributes declarations.
///
/// The caller normally obtains these runtime paths from
/// [`crate::paths::resolve_workspace_paths`]. Containment is checked again before
/// each read. Directory symlinks are rejected to avoid ambiguous module names
/// and cycles. The semantic pass owns imports, exports, decorator meaning, and
/// final item identities; this pass claims only the capabilities it implements.
pub fn parse_target(
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
    target: &ResolvedTargetPath,
) -> ParsedPythonPackage {
    let reference = TargetReference {
        package: package.id.clone(),
        target: target.id.clone(),
    };
    let observation = ExtractionObservation {
        target: reference.clone(),
        extractor: BuiltinExtractor::Python,
        capabilities: BTreeSet::new(),
        parsers: BTreeMap::new(),
        inputs: BTreeMap::new(),
    };
    let mut result = ParsedPythonPackage {
        package: package.id.clone(),
        target: reference,
        metadata: None,
        modules: vec![],
        inputs: BTreeMap::new(),
        diagnostics: vec![],
        provenance: observation.clone().into_provenance(None, None),
    };
    if !package.path.starts_with(&repository.path)
        || !target.path.starts_with(&package.path)
        || fs::canonicalize(&package.path).ok().as_ref() != Some(&package.path)
        || fs::canonicalize(&target.path).ok().as_ref() != Some(&target.path)
    {
        result.diagnostics.push(Diagnostic::new(DiagnosticCode::SourcePathOutsideBoundary, Severity::Error,
            "Python target and package roots must be canonical paths contained in the configured repository."));
        return result;
    }
    let metadata_input = read_input(
        repository,
        package,
        &package.metadata_path,
        PythonInputKind::Metadata,
        &mut result,
    );
    let mut selected_grammar = None;
    if let Some(input) = metadata_input {
        result.metadata = metadata::parse(
            &input.text,
            &input.source,
            &mut result.diagnostics,
            &mut selected_grammar,
        );
    }
    let mut files = Vec::new();
    discover(&target.path, &mut files, &mut result.diagnostics);
    files.sort();
    // Unknown metadata cannot authorize a version-specific surface. Use the
    // pinned parser's explicit supported ceiling only to collect syntax errors.
    let grammar = selected_grammar.as_deref().unwrap_or("3.14").to_owned();
    let mut module_paths: BTreeMap<(String, bool), Vec<usize>> = BTreeMap::new();
    for path in files {
        let kind = if path.file_name().is_some_and(|name| name == "py.typed") {
            PythonInputKind::TypedMarker
        } else {
            match path.extension().and_then(|value| value.to_str()) {
                Some("py") => PythonInputKind::Source,
                Some("pyi") => PythonInputKind::Stub,
                _ => continue,
            }
        };
        let Some(input) = read_input(repository, package, &path, kind, &mut result) else {
            continue;
        };
        if kind == PythonInputKind::TypedMarker {
            continue;
        }
        let Some((name, is_package)) = module_name(&path, &target.path, &package.path, &grammar)
        else {
            result.diagnostics.push(diagnostic(
                DiagnosticCode::PythonSourceRead,
                Severity::Error,
                "The source path does not establish a portable Python module name.",
                &input.source,
            ));
            continue;
        };
        let source_kind = if kind == PythonInputKind::Stub {
            PythonSourceKind::Stub
        } else {
            PythonSourceKind::Source
        };
        let index = result.modules.len();
        module_paths
            .entry((name.clone(), source_kind == PythonSourceKind::Stub))
            .or_default()
            .push(index);
        let mut module = source::parse(
            &input.text,
            name,
            is_package,
            source_kind,
            input.source,
            &grammar,
            &mut result.diagnostics,
        );
        if selected_grammar.is_none() {
            invalidate(&mut module);
        }
        result.modules.push(module);
    }
    for ((name, _), indexes) in module_paths {
        if indexes.len() > 1 {
            for index in indexes {
                let module = &mut result.modules[index];
                invalidate(module);
                result.diagnostics.push(diagnostic(
                    DiagnosticCode::PythonModuleCollision,
                    Severity::Error,
                    format!("More than one input defines the module variant `{name}`."),
                    &module.source,
                ));
            }
        }
    }
    result
        .modules
        .sort_by(|left, right| left.source.path.cmp(&right.source.path));
    result.diagnostics.sort();
    result.provenance = provenance(observation, &result, &grammar, selected_grammar.is_some())
        .into_provenance(None, None);
    result
}

fn invalidate(module: &mut ParsedModule) {
    module.valid = false;
    module.declarations.clear();
    module.imports.clear();
    module.exports.clear();
    module.docstring = None;
}

fn read_input(
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
    path: &Path,
    kind: PythonInputKind,
    result: &mut ParsedPythonPackage,
) -> Option<PythonInput> {
    let declared = match path.strip_prefix(&package.path) {
        Ok(path) => path,
        Err(_) => {
            result.diagnostics.push(Diagnostic::new(
                DiagnosticCode::SourcePathOutsideBoundary,
                Severity::Error,
                "A Python input escapes the configured package.",
            ));
            return None;
        }
    };
    let path = match resolve_input_file(&package.path, "python.input", declared, &package.path) {
        Ok(path) => path,
        Err(error) => {
            result
                .diagnostics
                .push(path_diagnostic(&error, repository, path));
            return None;
        }
    };
    let source = match repository.source_location(&path, None) {
        Ok(source) => source,
        Err(error) => {
            result
                .diagnostics
                .push(path_diagnostic(&error, repository, &path));
            return None;
        }
    };
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            result.diagnostics.push(diagnostic(
                DiagnosticCode::PythonSourceRead,
                Severity::Error,
                format!("Cannot read Python input: {:?}.", error.kind()),
                &source,
            ));
            return None;
        }
    };
    let fingerprint = fingerprint_bytes(&bytes);
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            result.diagnostics.push(diagnostic(
                DiagnosticCode::PythonSourceRead,
                Severity::Error,
                "Python inputs must use UTF-8 encoding.",
                &source,
            ));
            result.inputs.insert(
                source.path.clone(),
                PythonInput {
                    kind,
                    source,
                    text: String::new(),
                    raw_bytes: Some(error.into_bytes()),
                    fingerprint,
                },
            );
            return None;
        }
    };
    if text.len() > u32::MAX as usize {
        result.diagnostics.push(diagnostic(
            DiagnosticCode::PythonSourceRead,
            Severity::Error,
            "Python input exceeds the parser's byte-range limit.",
            &source,
        ));
        return None;
    }
    let input = PythonInput {
        kind,
        source,
        text,
        raw_bytes: None,
        fingerprint,
    };
    result
        .inputs
        .insert(input.source.path.clone(), input.clone());
    Some(input)
}

fn discover(path: &Path, files: &mut Vec<PathBuf>, diagnostics: &mut Vec<Diagnostic>) {
    let failure = |diagnostics: &mut Vec<Diagnostic>, message: &str| {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::PythonSourceRead,
            Severity::Error,
            message,
        ));
    };
    let Ok(metadata) = fs::symlink_metadata(path) else {
        failure(diagnostics, "Cannot inspect a configured Python input.");
        return;
    };
    if metadata.is_symlink() {
        if fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
            files.push(path.into());
        } else {
            failure(
                diagnostics,
                "Directory and broken symlinks in Python targets are unsupported.",
            );
        }
    } else if metadata.is_file() {
        files.push(path.into());
    } else if metadata.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            failure(diagnostics, "Cannot list a Python source directory.");
            return;
        };
        let mut children = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => {
                    if !entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.starts_with('.') || name == "__pycache__")
                    {
                        children.push(entry.path());
                    }
                }
                Err(_) => failure(
                    diagnostics,
                    "Cannot inspect an entry in a Python source directory.",
                ),
            }
        }
        children.sort();
        for child in children {
            discover(&child, files, diagnostics);
        }
    }
}

fn module_name(
    path: &Path,
    target: &Path,
    package: &Path,
    grammar: &str,
) -> Option<(String, bool)> {
    let is_package = path.file_stem()? == "__init__";
    let (root, prefix) = if target.is_dir() {
        (target, vec![target.file_name()?.to_str()?.to_owned()])
    } else {
        let mut root = path.parent()?;
        while root != package
            && (root.join("__init__.py").is_file() || root.join("__init__.pyi").is_file())
        {
            root = root.parent()?;
        }
        (root, vec![])
    };
    let mut components = prefix;
    let relative = path.strip_prefix(root).ok()?;
    for part in relative.parent()?.components() {
        components.push(part.as_os_str().to_str()?.to_owned());
    }
    if !is_package {
        components.push(path.file_stem()?.to_str()?.to_owned());
    }
    if components.is_empty() && is_package {
        components.push(path.parent()?.file_name()?.to_str()?.to_owned());
    }
    if components
        .iter()
        .any(|part| !module_identifier(part, grammar))
    {
        return None;
    }
    Some((components.join("."), is_package))
}

fn module_identifier(name: &str, grammar: &str) -> bool {
    use ruff_python_ast::{Expr, Mod};
    use ruff_python_parser::{Mode, ParseOptions, parse};
    use ruff_text_size::Ranged;
    let options = ParseOptions::from(Mode::Expression)
        .with_target_version(grammar.parse().expect("validated grammar"));
    parse(name, options).is_ok_and(|parsed| {
        matches!(parsed.syntax(),
        Mod::Expression(expression) if matches!(&*expression.body,
            Expr::Name(value) if usize::from(value.start()) == 0
                && usize::from(value.end()) == name.len()))
    })
}

fn provenance(
    mut observation: ExtractionObservation,
    result: &ParsedPythonPackage,
    grammar: &str,
    grammar_known: bool,
) -> ExtractionObservation {
    observation.capabilities.insert("provenance.source".into());
    let metadata_used = result
        .inputs
        .values()
        .any(|input| input.kind == PythonInputKind::Metadata && input.raw_bytes.is_none());
    let syntax_used = result.inputs.values().any(|input| {
        matches!(input.kind, PythonInputKind::Source | PythonInputKind::Stub)
            && input.raw_bytes.is_none()
    });
    if metadata_used {
        observation
            .capabilities
            .insert("python.metadata.pep621".into());
    }
    for (name, version, role) in [
        ("pyproject-toml", "0.13.7", "python-metadata"),
        ("ruff_python_parser", "0.0.12", "python-syntax"),
        ("ruff_python_ast", "0.0.12", "python-ast"),
        ("ruff_text_size", "0.0.12", "source-ranges"),
    ] {
        if (name == "pyproject-toml" && !metadata_used)
            || (name != "pyproject-toml" && !syntax_used)
        {
            continue;
        }
        let mut settings = BTreeMap::new();
        if name == "ruff_python_parser" {
            settings.insert(
                "module_name_validation".into(),
                "identifier-expression".into(),
            );
            settings.insert("target_version".into(), grammar.into());
            settings.insert(
                "target_version_origin".into(),
                if grammar_known {
                    "metadata"
                } else {
                    "diagnostic-only-invalid-metadata"
                }
                .into(),
            );
            for module in &result.modules {
                settings.insert(
                    format!("source_type:{}", module.source.path.as_str()),
                    if module.kind == PythonSourceKind::Stub {
                        "stub"
                    } else {
                        "python"
                    }
                    .into(),
                );
            }
        }
        observation.parsers.insert(
            name.into(),
            ParserProvenance {
                version: version.into(),
                role: role.into(),
                settings,
            },
        );
    }
    for (path, input) in &result.inputs {
        let (kind, mut parsers) = match input.kind {
            PythonInputKind::Metadata => (
                "package-metadata",
                BTreeSet::from(["pyproject-toml".into()]),
            ),
            PythonInputKind::Source => ("python-source", syntax_parsers()),
            PythonInputKind::Stub => ("python-stub", syntax_parsers()),
            PythonInputKind::TypedMarker => ("python-typed-marker", BTreeSet::new()),
        };
        if input.raw_bytes.is_some()
            || (matches!(input.kind, PythonInputKind::Source | PythonInputKind::Stub)
                && !result
                    .modules
                    .iter()
                    .any(|module| &module.source.path == path))
        {
            parsers.clear();
        }
        observation
            .inputs
            .entry(input.source.repository.clone())
            .or_default()
            .insert(
                path.clone(),
                ExtractionInput {
                    kind: kind.into(),
                    fingerprint: input.fingerprint.clone(),
                    parsers,
                },
            );
    }
    observation
}

fn syntax_parsers() -> BTreeSet<String> {
    ["ruff_python_ast", "ruff_python_parser", "ruff_text_size"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn located(source: &SourceLocation, range: TextRange) -> SourceLocation {
    SourceLocation {
        span: Some(SourceSpan {
            start: usize::from(range.start()),
            end: usize::from(range.end()),
        }),
        ..source.clone()
    }
}

fn diagnostic(
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

fn path_diagnostic(
    error: &PathResolutionError,
    repository: &ResolvedRepositoryPaths,
    path: &Path,
) -> Diagnostic {
    // Reuse stable error classification, then replace configuration context
    // with only a proven portable repository spelling.
    let mut diagnostic =
        error.to_diagnostic("python-input".try_into().expect("fixed portable path"));
    diagnostic.source = path
        .strip_prefix(&repository.path)
        .ok()
        .and_then(|path| {
            crate::paths::portable_relative_path(&repository.path, "python.input", path).ok()
        })
        .map(|path| DiagnosticSource::Repository {
            repository: repository.id.clone(),
            path,
        });
    diagnostic.related_entity = None;
    diagnostic
}
