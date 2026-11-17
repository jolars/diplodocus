//! Normalize versioned native observations without starting a language process.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{
    ParsedDeclaration, ParsedPythonPackage, PythonExtraction, PythonSourceKind,
    docstrings::DocstringParse, references::PythonReferences, source, surface,
};
use crate::diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEntity, DiagnosticSource, Severity,
};
use crate::ir::*;
use crate::paths::{ResolvedPackagePaths, ResolvedRepositoryPaths};

/// Local exchange only: absolute module paths and environment owners are removed
/// before constructing portable IR. Unknown protocol fields are rejected.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observations {
    pub protocol: u32,
    pub python_version: String,
    pub sphinx_version: String,
    pub docutils_version: String,
    pub dependencies: BTreeMap<String, String>,
    pub modules: Vec<NativeModule>,
    pub docs: BTreeMap<String, DocNode>,
    pub raw_docs: BTreeMap<String, String>,
    pub doctest_sources: BTreeMap<String, String>,
    pub dataclass_fields: BTreeMap<String, Vec<String>>,
    pub doctest_modules: BTreeMap<String, Option<String>>,
    pub doctest_lines: BTreeMap<String, Vec<[usize; 2]>>,
    pub unsupported_defaults: BTreeSet<String>,
    pub unsupported_generics: BTreeSet<String>,
    pub warnings: String,
    pub environment: serde_json::Value,
    pub inputs: BTreeMap<PathBuf, Fingerprint>,
    pub input_aliases: BTreeMap<PathBuf, PathBuf>,
    pub inputs_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeModule {
    pub name: String,
    pub path: String,
    pub exports: Option<Vec<String>>,
    pub declarations: Vec<NativeDeclaration>,
    pub support_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeDeclaration {
    name: String,
    kind: String,
    signatures: Vec<String>,
    overload_lines: Vec<Option<usize>>,
    decorators: Vec<String>,
    bases: Vec<String>,
    members: Vec<NativeDeclaration>,
    is_async: bool,
    alias: Option<String>,
    assignment_alias: bool,
    implicit_alias: bool,
    annotation: Option<String>,
    value: Option<String>,
    has_setter: bool,
    has_deleter: bool,
    source_line: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DoctestPlan {
    /// Importable module that supplies the docstring's globals.
    pub module: String,
    /// Half-open line ranges in the cleaned original docstring.
    pub lines: Vec<[usize; 2]>,
    /// Original docstring text, used only in the local execution request.
    pub source: String,
}

/// Executable doctests keyed by normalized item ID.
pub(crate) type DoctestPlans = BTreeMap<String, DoctestPlan>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DocNode {
    kind: String,
    text: String,
    children: Vec<DocNode>,
    #[serde(default)]
    reftarget: Option<String>,
    #[serde(default)]
    refuri: Option<String>,
    #[serde(default)]
    refdomain: Option<String>,
    #[serde(default)]
    reftype: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

/// Structured observations use the same identity and stub-normalization helpers
/// as maintained declarations. Python has already decided which runtime
/// bindings, descriptors, decorators, exports, and class shapes actually exist.
pub(crate) fn normalize(
    mut parsed: ParsedPythonPackage,
    observed: &Observations,
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
) -> Result<(PythonExtraction, Provenance, DoctestPlans, BTreeSet<String>), String> {
    if observed.protocol != 6 {
        return Err("Unsupported Python exporter protocol.".into());
    }
    let originals = parsed.modules.clone();
    // Unsupported *static semantics* do not invalidate a native observation.
    // Syntax, boundary, metadata, and maintained-stub failures still do.
    parsed.diagnostics = std::mem::take(&mut parsed.diagnostics)
        .into_iter()
        .filter(|diagnostic| !supplemental_diagnostic(&parsed, diagnostic))
        .collect();
    parsed
        .modules
        .retain(|module| module.kind == PythonSourceKind::Stub);
    let mut native_names = BTreeSet::new();
    let mut data_descriptors = BTreeSet::new();
    for module in &observed.modules {
        if !native_names.insert(module.name.clone()) {
            return Err("Duplicate native Python module.".into());
        }
        let path = std::fs::canonicalize(&module.path).map_err(|_| "Native module disappeared.")?;
        if !path.starts_with(&package.path) {
            return Err("Native module escapes the selected package.".into());
        }
        let location = repository
            .source_location(&path, None)
            .map_err(|_| "Native module escapes its repository.")?;
        let original = originals
            .iter()
            .find(|m| m.name == module.name && m.kind == PythonSourceKind::Source);
        let mut text = String::from("import typing\n");
        // Imported names qualify annotations only. Runtime aliases below are
        // authoritative for the documentation surface.
        if let Some(original) = original {
            for import in &original.imports {
                if import.names.iter().any(|name| name.name == "*") {
                    continue;
                }
                text.push_str("if typing.TYPE_CHECKING:\n    ");
                if import.module.is_some() || import.level > 0 {
                    text.push_str("from ");
                    text.push_str(&".".repeat(import.level as usize));
                    text.push_str(import.module.as_deref().unwrap_or_default());
                    text.push_str(" import ");
                } else {
                    text.push_str("import ");
                }
                text.push_str(
                    &import
                        .names
                        .iter()
                        .map(|name| {
                            name.alias.as_ref().map_or_else(
                                || name.name.clone(),
                                |alias| format!("{} as {alias}", name.name),
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                text.push('\n');
            }
        }
        for declaration in &module.declarations {
            render(
                declaration,
                &module.name,
                0,
                &mut text,
                &mut data_descriptors,
            )?;
        }
        let mut diagnostics = vec![];
        let mut normalized = source::parse_native(
            &text,
            module.name.clone(),
            original.is_some_and(|m| m.is_package),
            location.clone(),
            &mut diagnostics,
        );
        if !normalized.valid {
            return Err(format!(
                "Native Python signatures cannot be represented: {}",
                diagnostics
                    .iter()
                    .map(|d| d.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        normalized.source = location.clone();
        // Generated source is a parsing aid, never source evidence.
        normalized.docstring = original.and_then(|m| m.docstring.clone());
        let source_text = original
            .and_then(|module| parsed.inputs.get(&module.source.path))
            .map(|input| input.text.as_str());
        remap_declarations(
            &mut normalized.declarations,
            &module.declarations,
            original
                .map(|m| m.declarations.as_slice())
                .unwrap_or_default(),
            &location,
            source_text,
        );
        for import in &mut normalized.imports {
            import.source = location.clone();
            if !import.annotation_only
                && let (Some(original), Some(source_text)) = (original, source_text)
                && let [name] = import.names.as_slice()
                && let local = name.alias.as_deref().unwrap_or(&name.name)
                && let Some(line) = module
                    .declarations
                    .iter()
                    .find(|d| d.name == local && d.alias.is_some() && !d.assignment_alias)
                    .and_then(|d| d.source_line)
                && let Some(proven) = original.imports.iter().find(|import| {
                    !import.annotation_only
                        && within_lines(source_text, &import.source, line)
                        && import.names.iter().any(|name| {
                            name.alias
                                .as_deref()
                                .unwrap_or_else(|| name.name.split('.').next().unwrap())
                                == local
                        })
                })
            {
                import.source = proven.source.clone();
            }
        }
        parsed.modules.push(normalized);
    }
    let support = observed
        .modules
        .iter()
        .filter(|module| module.support_only)
        .map(|module| module.name.clone())
        .collect();
    let exports = observed
        .modules
        .iter()
        .filter_map(|module| {
            module
                .exports
                .as_deref()
                .map(|names| (module.name.as_str(), names))
        })
        .collect();
    let mut result = surface::reconcile_with_runtime(
        &parsed,
        &support,
        &exports,
        &data_descriptors,
        &observed.dataclass_fields,
    );
    let module_names: BTreeSet<_> = parsed
        .modules
        .iter()
        .map(|module| module.name.as_str())
        .collect();
    let support_items = result
        .items
        .iter()
        .filter(|(_, item)| {
            // A selected submodule can have a support-only ancestor. Use
            // the closest module, which owns the canonical declaration.
            std::iter::successors(Some(item.qualified_name.as_str()), |name| {
                name.rsplit_once('.').map(|(parent, _)| parent)
            })
            .find(|name| module_names.contains(name))
            .is_some_and(|name| support.contains(name))
        })
        .map(|(id, _)| id.clone())
        .collect();
    if !observed.warnings.trim().is_empty() {
        result.diagnostics.push(super::diagnostic(
            DiagnosticCode::PythonUnsupportedDocstring,
            Severity::Warning,
            observed.warnings.trim(),
            &parsed
                .metadata
                .as_ref()
                .ok_or("Missing Python metadata.")?
                .source,
        ));
    }
    let references = PythonReferences::new(&result.items);
    for item in result.items.values_mut() {
        if let Some(ItemLanguageData::Python(data)) = &mut item.language_data
            && let PythonDeclaration::Module { source, .. } = &mut data.declaration
            && !originals.iter().any(|m| m.name == item.qualified_name)
        {
            *source = PythonModuleSource::Runtime;
        }
    }
    let mut doctests = DoctestPlans::new();
    for (id, item) in &mut result.items {
        if let Some(name) = std::iter::once(&item.qualified_name)
            .chain(item.aliases.iter().map(|alias| &alias.qualified_name))
            .find(|name| observed.unsupported_generics.contains(*name))
        {
            return Err(format!(
                "Generic type parameters are not yet represented in Python API records: {name}"
            ));
        }
        if observed.unsupported_defaults.contains(&item.qualified_name)
            && let Some(location) = &item.source_location
        {
            result.diagnostics.push(super::diagnostic(
                DiagnosticCode::PythonUnsupportedSurface,
                Severity::Warning,
                format!(
                    "The runtime default for `{}` cannot be represented and is shown as `...`.",
                    item.qualified_name
                ),
                location,
            ));
        }
        if matches!(
            &item.language_data,
            Some(ItemLanguageData::Python(PythonItemData {
                declaration: PythonDeclaration::Callable {
                    role: PythonCallableRole::Overload { .. },
                    ..
                },
                ..
            }))
        ) {
            continue;
        }
        if item.signatures.is_empty()
            && matches!(
                &item.language_data,
                Some(ItemLanguageData::Python(PythonItemData {
                    declaration: PythonDeclaration::Callable { .. },
                    ..
                }))
            )
            && let Some(location) = &item.source_location
        {
            result.diagnostics.push(super::diagnostic(DiagnosticCode::PythonUnsupportedSurface, Severity::Warning,
                format!("Runtime callable `{}` has no representable signature; provide a maintained stub for its public signature.", item.qualified_name), location));
        }
        if matches!(
            &item.language_data,
            Some(ItemLanguageData::Python(PythonItemData {
                declaration: PythonDeclaration::Module { .. },
                visibility: PythonVisibility::Private,
                ..
            }))
        ) {
            continue;
        }
        let name = &item.qualified_name;
        let aliases = item
            .aliases
            .iter()
            .map(|alias| alias.qualified_name.as_str());
        let key = std::iter::once(name.as_str())
            .chain(aliases)
            .find(|name| observed.docs.contains_key(*name));
        let Some(key) = key else {
            continue;
        };
        let raw = observed.raw_docs.get(key).cloned().unwrap_or_default();
        if raw.is_empty() {
            continue;
        }
        let doctest_source = observed.doctest_sources.get(key);
        let original = result.docstrings.get(id).filter(|doc| {
            clean(&doc.text) == clean(&raw) && doctest_source.is_none_or(|source| source == &raw)
        });
        if let Some(lines) = observed.doctest_lines.get(key)
            && !lines.is_empty()
            && let Some(module) = observed
                .doctest_modules
                .get(key)
                .ok_or("Missing defining module for a native doctest.")?
        {
            doctests.insert(
                id.clone(),
                DoctestPlan {
                    module: module.clone(),
                    lines: lines.clone(),
                    source: original
                        .map(|doc| doc.text.clone())
                        .or_else(|| doctest_source.cloned())
                        .ok_or("Missing original native doctest source.")?,
                },
            );
        }
        let raw = original.map(|doc| doc.text.clone()).unwrap_or(raw);
        let location = original
            .map(|doc| doc.source.clone())
            .or_else(|| item.source_location.clone())
            .ok_or("Native documentation has no owning module.")?;
        let span = SourceSpan {
            start: 0,
            end: raw.len(),
        };
        let mut warnings = BTreeSet::new();
        let blocks = blocks(&observed.docs[key], span, &mut warnings);
        if raw.lines().any(|line| {
            line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~")
        }) {
            warnings.insert("Markdown fences are unsupported in Sphinx docstrings".into());
        }
        if raw.lines().any(|line| {
            let line = line.trim_start();
            let heading = line.trim_start_matches('#');
            (heading.len() < line.len()
                && line.len() - heading.len() <= 6
                && heading.starts_with(' '))
                || (line.contains('[') && line.contains("]("))
        }) {
            warnings
                .insert("Markdown headings and links are unsupported in Sphinx docstrings".into());
        }
        let mut document = DocstringParse {
            document: SourcedDocument {
                document: Document {
                    span,
                    frontmatter: None,
                    blocks,
                },
                source_format: DocumentFormat::Extracted {
                    name: "python-docstring".into(),
                },
                source_location: Some(location.clone()),
                generated_source: None,
                raw_source: Some(raw),
                provenance: vec![Provenance {
                    activity: ProvenanceActivity::Declaration,
                    source: Some(DiagnosticSource::Repository {
                        repository: location.repository.clone(),
                        path: location.path.clone(),
                    }),
                    span: location.span,
                    tools: BTreeMap::from([
                        ("sphinx".into(), observed.sphinx_version.clone()),
                        ("docutils".into(), observed.docutils_version.clone()),
                    ]),
                }],
            },
            diagnostics: warnings
                .into_iter()
                .map(|message| {
                    super::diagnostic(
                        DiagnosticCode::PythonUnsupportedDocstring,
                        Severity::Warning,
                        message,
                        &location,
                    )
                })
                .collect(),
            source_map: vec![],
        };
        references.normalize(&mut document, name);
        for diagnostic in &mut document.diagnostics {
            diagnostic.related_entity = Some(DiagnosticEntity::Item {
                package: parsed.package.clone(),
                id: id.clone(),
            });
        }
        result.diagnostics.extend(document.diagnostics);
        item.documentation = Some(document.document);
    }
    let mut provenance = parsed.provenance;
    let mut inputs = vec![];
    if let ProvenanceActivity::Extraction {
        mode,
        capabilities,
        parsers,
        inputs: files,
        ..
    } = &mut provenance.activity
    {
        *mode = ExtractionMode::Native;
        capabilities.extend(
            [
                "python.declarations.native",
                "python.exports.runtime",
                "python.stubs",
                "python.overloads",
                "python.docs.sphinx",
                "python.docs.numpy",
                "python.docs.google",
                "diagnostics.unsupported-visible",
            ]
            .map(str::to_owned),
        );
        for (name, version) in [
            ("sphinx", &observed.sphinx_version),
            ("docutils", &observed.docutils_version),
        ] {
            provenance.tools.insert(name.into(), version.clone());
            parsers.insert(
                name.into(),
                ParserProvenance {
                    version: version.clone(),
                    role: "python-native-documentation".into(),
                    settings: BTreeMap::new(),
                },
            );
        }
        provenance
            .tools
            .insert("python".into(), observed.python_version.clone());
        for (path, fingerprint) in &observed.inputs {
            let path = std::fs::canonicalize(path).map_err(|_| "Native input disappeared.")?;
            let location = repository
                .source_location(&path, None)
                .map_err(|_| "Native input escapes the repository.")?;
            if !path.starts_with(&package.path) {
                return Err("Native input escapes the selected package.".into());
            }
            files
                .entry(location.repository)
                .or_default()
                .entry(location.path)
                .or_insert(ExtractionInput {
                    kind: "native-input".into(),
                    fingerprint: fingerprint.clone(),
                    parsers: BTreeSet::from(["sphinx".into()]),
                })
                .parsers
                .insert("sphinx".into());
        }
        for (repository, files) in files {
            for (path, input) in files {
                inputs.push(InputFingerprint {
                    source: SourceLocation {
                        repository: repository.clone(),
                        path: path.clone(),
                        span: None,
                    },
                    fingerprint: input.fingerprint.clone(),
                });
            }
        }
    }
    let completion = Provenance {
        activity: ProvenanceActivity::PythonRuntime {
            target: parsed.target.clone(),
            policy: "python-sphinx-v1".into(),
            python_version: observed.python_version.clone(),
            sphinx_version: observed.sphinx_version.clone(),
            docutils_version: observed.docutils_version.clone(),
            package_version: parsed
                .metadata
                .as_ref()
                .ok_or("Missing Python metadata.")?
                .version
                .clone(),
            dependencies: observed.dependencies.clone(),
            inputs,
        },
        source: provenance.source.clone(),
        span: None,
        tools: provenance.tools.clone(),
    };
    result.diagnostics.sort();
    result.diagnostics.dedup();
    Ok((
        PythonExtraction {
            schema_version: SchemaVersion,
            package: parsed.package,
            target: parsed.target,
            metadata: parsed.metadata,
            items: result.items,
            diagnostics: result.diagnostics,
            provenance,
        },
        completion,
        doctests,
        support_items,
    ))
}

pub(crate) fn supplemental_diagnostic(
    parsed: &ParsedPythonPackage,
    diagnostic: &Diagnostic,
) -> bool {
    matches!(
        diagnostic.code,
        DiagnosticCode::PythonUnsupportedSurface
            | DiagnosticCode::PythonDynamicExport
            | DiagnosticCode::PythonUnsupportedSyntax
            | DiagnosticCode::PythonUnsupportedVersion
    ) && parsed.modules.iter().any(|module| {
        module.kind == PythonSourceKind::Source
            && matches!(&diagnostic.source, Some(DiagnosticSource::Repository { repository, path })
                if repository == &module.source.repository && path == &module.source.path)
    })
}

fn clean(text: &str) -> String {
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| {
            let mut expanded = String::new();
            let mut column = 0;
            for ch in line.chars() {
                if ch == '\t' {
                    let width = 8 - column % 8;
                    expanded.extend(std::iter::repeat_n(' ', width));
                    column += width;
                } else {
                    expanded.push(ch);
                    column += 1;
                }
            }
            expanded
        })
        .collect();
    let margin = lines
        .iter()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.chars().take_while(|ch| ch.is_whitespace()).count())
        .min()
        .unwrap_or(0);
    if let Some(first) = lines.first_mut() {
        *first = first.trim_start().to_owned();
    }
    for line in lines.iter_mut().skip(1) {
        *line = line.chars().skip(margin).collect();
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    let start = lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(lines.len());
    lines[start..].join("\n")
}

fn render(
    declaration: &NativeDeclaration,
    parent: &str,
    depth: usize,
    out: &mut String,
    data_descriptors: &mut BTreeSet<String>,
) -> Result<(), String> {
    let prefix = "    ".repeat(depth);
    let name = &declaration.name;
    if !ruff_python_parser::parse_expression(name).is_ok_and(|parsed| {
        matches!(parsed.syntax().body.as_ref(), ruff_python_ast::Expr::Name(binding)
            if usize::from(binding.range.len()) == name.len())
    }) {
        return Err("Invalid runtime Python binding name.".into());
    }
    if let Some(alias) = &declaration.alias {
        if depth == 0 && !declaration.assignment_alias {
            let as_name = if declaration.implicit_alias {
                String::new()
            } else {
                format!(" as {name}")
            };
            if let Some((parent, original)) = alias.rsplit_once('.') {
                out.push_str(&format!("from {parent} import {original}{as_name}\n"));
            } else {
                out.push_str(&format!("import {alias}{as_name}\n"));
            }
        } else {
            out.push_str(&format!("{prefix}{name} = {alias}\n"));
        }
        return Ok(());
    }
    for decorator in &declaration.decorators {
        out.push_str(&format!("{prefix}@{decorator}\n"));
    }
    match declaration.kind.as_str() {
        "class" => {
            out.push_str(&format!(
                "{prefix}class {name}({}):\n{prefix}    pass\n",
                declaration.bases.join(", ")
            ));
            let qualified = format!("{parent}.{name}");
            for child in &declaration.members {
                render(child, &qualified, depth + 1, out, data_descriptors)?;
            }
        }
        "function" => {
            let overloaded = !declaration.overload_lines.is_empty();
            let signatures: Vec<_> = if declaration.signatures.is_empty() {
                vec!["(*args, **kwargs)"]
            } else {
                declaration.signatures.iter().map(String::as_str).collect()
            };
            let asynchronous = if declaration.is_async { "async " } else { "" };
            for signature in &signatures {
                if overloaded {
                    out.push_str(&format!("{prefix}@typing.overload\n"));
                }
                out.push_str(&format!(
                    "{prefix}{asynchronous}def {name}{signature}: ...\n"
                ));
            }
            if overloaded {
                out.push_str(&format!(
                    "{prefix}{asynchronous}def {name}(*args, **kwargs): ...\n"
                ));
            }
            for (present, accessor, signature) in [
                (declaration.has_setter, "setter", "(self, value)"),
                (declaration.has_deleter, "deleter", "(self)"),
            ] {
                if present {
                    out.push_str(&format!(
                        "{prefix}@{name}.{accessor}\n{prefix}def {name}{signature}: ...\n"
                    ));
                }
            }
        }
        "type-alias" => out.push_str(&format!(
            "{prefix}{name}: typing.TypeAlias = {}\n",
            declaration.value.as_deref().unwrap_or("...")
        )),
        "value" | "data-descriptor" => {
            if declaration.kind == "data-descriptor" {
                data_descriptors.insert(format!("{parent}.{name}"));
            }
            out.push_str(&format!("{prefix}{name}"));
            if let Some(annotation) = &declaration.annotation {
                out.push_str(&format!(": {annotation}"));
            }
            if let Some(value) = &declaration.value {
                out.push_str(&format!(" = {value}"));
            } else if declaration.annotation.is_none() {
                out.push_str(" = ...");
            }
            out.push('\n');
        }
        _ => {
            return Err(format!(
                "Unsupported native Python declaration in {parent}."
            ));
        }
    }
    Ok(())
}

fn within_lines(text: &str, location: &SourceLocation, line: usize) -> bool {
    let Some(span) = location.span else {
        return false;
    };
    let line_at = |offset| {
        text.get(..offset).map(|prefix| {
            prefix
                .as_bytes()
                .iter()
                .filter(|&&byte| byte == b'\n')
                .count()
                + 1
        })
    };
    line_at(span.start)
        .zip(line_at(span.end))
        .is_some_and(|(start, end)| (start..=end).contains(&line))
}

fn remap_declarations(
    declarations: &mut [ParsedDeclaration],
    native: &[NativeDeclaration],
    originals: &[ParsedDeclaration],
    fallback: &SourceLocation,
    source_text: Option<&str>,
) {
    let mut overload_index = BTreeMap::new();
    for declaration in declarations {
        let observed = native.iter().find(|d| d.name == declaration.name);
        let line = observed.and_then(|observed| {
            if declaration.decorators.iter().any(|decorator| {
                matches!(&decorator.expression, SignatureExpression::Name { name, .. } if name == "typing.overload")
            }) {
                let index = overload_index.entry(declaration.name.clone()).or_insert(0);
                let line = observed.overload_lines.get(*index).copied().flatten();
                *index += 1;
                line
            } else {
                observed.source_line
            }
        });
        let matching = |original: &&ParsedDeclaration| {
            original.name == declaration.name && original.kind == declaration.kind
        };
        let proven = line.zip(source_text).and_then(|(line, text)| {
            originals
                .iter()
                .filter(matching)
                .find(|original| within_lines(text, &original.source, line))
        });
        let original = proven
            .or_else(|| {
                originals.iter().filter(matching).find(|original| {
                    original
                        .signature
                        .as_ref()
                        .zip(declaration.signature.as_ref())
                        .is_some_and(|(left, right)| left.signature == right.signature)
                })
            })
            .or_else(|| originals.iter().find(matching));
        declaration.source = proven
            .map(|d| d.source.clone())
            .unwrap_or_else(|| fallback.clone());
        declaration.docstring = originals
            .iter()
            .filter(matching)
            .find_map(|d| d.docstring.clone());
        if let Some(signature) = &mut declaration.signature {
            signature.sources = vec![SourceEvidence {
                source: original
                    .and_then(|original| original.signature.as_ref())
                    .filter(|original| original.signature == signature.signature)
                    .and_then(|original| original.sources.first())
                    .map(|source| source.source.clone())
                    .unwrap_or_else(|| fallback.clone()),
                role: SourceRole::Signature,
                parsers: BTreeSet::from(["sphinx".into(), "ruff_python_parser".into()]),
            }];
            if let (Some(observed), Signature::Value { annotation, value }) =
                (observed, &mut signature.signature)
            {
                if observed.annotation.is_none() {
                    *annotation = None;
                }
                if observed.value.is_none() && observed.alias.is_none() {
                    *value = None;
                }
            }
        }
        if observed.is_some_and(|d| d.kind == "function" && d.signatures.is_empty()) {
            declaration.signature = None;
        }
        for decorator in &mut declaration.decorators {
            decorator.source = declaration.source.clone();
        }
        remap_declarations(
            &mut declaration.members,
            observed.map(|d| d.members.as_slice()).unwrap_or_default(),
            original.map(|d| d.members.as_slice()).unwrap_or_default(),
            &declaration.source,
            source_text,
        );
    }
}

fn inlines(node: &DocNode, span: SourceSpan, warnings: &mut BTreeSet<String>) -> Vec<Inline> {
    let children = |warnings: &mut BTreeSet<String>| {
        node.children
            .iter()
            .flat_map(|child| inlines(child, span, warnings))
            .collect()
    };
    vec![match node.kind.as_str() {
        "text" => Inline::Text {
            value: node.text.clone(),
            span,
        },
        "literal" | "literal_emphasis" | "literal_strong" => Inline::Code {
            value: node.text.clone(),
            span,
        },
        "emphasis" => Inline::Emphasis {
            inlines: children(warnings),
            span,
        },
        "strong" => Inline::Strong {
            inlines: children(warnings),
            span,
        },
        "math" => Inline::Math {
            latex: node.text.clone(),
            fallback: None,
            span,
        },
        "pending_xref" => Inline::Link {
            target: format!("@ref {}", node.reftarget.as_deref().unwrap_or(&node.text)),
            inlines: children(warnings),
            title: None,
            attributes: Attributes::default(),
            span,
        },
        "reference" if node.refuri.is_some() => Inline::Link {
            target: node.refuri.clone().unwrap(),
            inlines: children(warnings),
            title: None,
            attributes: Attributes::default(),
            span,
        },
        "inline" | "reference" | "problematic" | "citation_reference" | "footnote_reference" => {
            return if node.children.is_empty() {
                vec![Inline::Text {
                    value: node.text.clone(),
                    span,
                }]
            } else {
                children(warnings)
            };
        }
        _ => {
            warnings.insert(format!("Unsupported Sphinx inline node: {}", node.kind));
            Inline::Unsupported {
                source_kind: node.kind.clone(),
                raw: node.text.clone(),
                span,
            }
        }
    }]
}

/// Sphinx's Python domain groups typed fields into paragraphs and bullet lists.
/// Preserve those entries as common description lists, including symbolic types.
fn field_entries(
    body: &DocNode,
    name: &str,
    span: SourceSpan,
    warnings: &mut BTreeSet<String>,
) -> Option<Block> {
    let parameters = matches!(
        name,
        "Parameters" | "Other Parameters" | "Keyword Arguments"
    );
    if !parameters
        && !matches!(
            name,
            "Returns" | "Yields" | "Raises" | "Attributes" | "Variables"
        )
    {
        return None;
    }
    let entries = if body.children.len() == 1 && body.children[0].kind == "bullet_list" {
        body.children[0].children.iter().collect::<Vec<_>>()
    } else {
        vec![body]
    };
    let mut definitions = Vec::new();
    for entry in entries {
        let paragraph = entry.children.first()?;
        if paragraph.kind != "paragraph" {
            return None;
        }
        let mut term: Vec<_> = paragraph
            .children
            .iter()
            .flat_map(|child| inlines(child, span, warnings))
            .collect();
        let separator = term.iter().enumerate().find_map(|(index, inline)| {
            let Inline::Text { value, .. } = inline else {
                return None;
            };
            value
                .split_once(" – ")
                .or_else(|| value.split_once(" -- "))
                .map(|(before, after)| (index, before.to_owned(), after.to_owned()))
        });
        let mut description = Vec::new();
        if let Some((index, before, after)) = separator {
            let mut prose = term.split_off(index + 1);
            term.pop();
            if !before.is_empty() {
                term.push(Inline::Text {
                    value: before,
                    span,
                });
            }
            if !after.is_empty() {
                prose.insert(0, Inline::Text { value: after, span });
            }
            if !prose.is_empty() {
                description.push(Block::Paragraph {
                    inlines: prose,
                    span,
                });
            }
        }
        if parameters {
            // The domain emits the type in parentheses after the parameter name.
            // Use the same name : type spelling as maintained API descriptions.
            for inline in &mut term {
                if let Inline::Text { value, .. } = inline
                    && value == " ("
                {
                    *value = " : ".into();
                }
            }
            if let Some(Inline::Text { value, .. }) = term.last_mut()
                && value == ")"
            {
                value.clear();
            }
        }
        description.extend(
            entry
                .children
                .iter()
                .skip(1)
                .flat_map(|child| blocks(child, span, warnings)),
        );
        definitions.push(DescriptionEntry {
            term: vec![Block::Paragraph {
                inlines: term,
                span,
            }],
            description,
            span,
        });
    }
    Some(Block::description_list(definitions, span))
}

fn blocks(node: &DocNode, span: SourceSpan, warnings: &mut BTreeSet<String>) -> Vec<Block> {
    let children = |warnings: &mut BTreeSet<String>| {
        node.children
            .iter()
            .flat_map(|child| blocks(child, span, warnings))
            .collect()
    };
    vec![match node.kind.as_str() {
        "desc_content" | "section" | "document" | "compound" | "definition" | "field_body" => {
            return children(warnings);
        }
        "paragraph" | "term" | "classifier" | "field_name" | "rubric" | "title" | "label" => {
            let inlines = node
                .children
                .iter()
                .flat_map(|child| inlines(child, span, warnings))
                .collect();
            if matches!(node.kind.as_str(), "rubric" | "title") {
                Block::Heading {
                    level: 2,
                    attributes: Attributes::default(),
                    inlines,
                    span,
                }
            } else {
                Block::Paragraph { inlines, span }
            }
        }
        "literal_block" | "doctest_block" => Block::CodeBlock {
            language: Some(node.language.clone().unwrap_or_else(|| "python".into())),
            source: node.text.clone(),
            source_segments: vec![SourceSegment {
                text: node.text.clone(),
                span,
            }],
            span,
        },
        "bullet_list" | "enumerated_list" => Block::List {
            ordered: node.kind == "enumerated_list",
            items: node
                .children
                .iter()
                .map(|item| ListItem {
                    checked: None,
                    blocks: blocks(item, span, warnings),
                    span,
                })
                .collect(),
            span,
        },
        "list_item" => return children(warnings),
        "field_list" => {
            let mut result = Vec::new();
            for field in &node.children {
                if let Some(name) = field.children.first() {
                    result.push(Block::Heading {
                        level: 2,
                        attributes: Attributes::default(),
                        inlines: vec![Inline::Text {
                            value: name.text.clone(),
                            span,
                        }],
                        span,
                    });
                }
                let definitions = field
                    .children
                    .get(1)
                    .and_then(|body| field_entries(body, &field.children[0].text, span, warnings));
                if let Some(definitions) = definitions {
                    result.push(definitions);
                } else {
                    result.extend(
                        field
                            .children
                            .iter()
                            .skip(1)
                            .flat_map(|node| blocks(node, span, warnings)),
                    );
                }
            }
            return result;
        }
        "definition_list" => Block::description_list(
            node.children
                .iter()
                .map(|entry| DescriptionEntry {
                    term: entry
                        .children
                        .first()
                        .map(|node| blocks(node, span, warnings))
                        .unwrap_or_default(),
                    description: entry
                        .children
                        .iter()
                        .skip(1)
                        .flat_map(|node| blocks(node, span, warnings))
                        .collect(),
                    span,
                })
                .collect(),
            span,
        ),
        "block_quote" => Block::BlockQuote {
            blocks: children(warnings),
            span,
        },
        "citation" | "footnote" => Block::Container {
            kind: if node.kind == "citation" {
                DocumentContainerKind::BibliographyEntry
            } else {
                DocumentContainerKind::Footnote
            },
            identifier: None,
            blocks: children(warnings),
            span,
        },
        "transition" => Block::ThematicBreak { span },
        "math_block" => Block::DisplayMath {
            latex: node.text.clone(),
            fallback: None,
            span,
        },
        "note" | "warning" | "tip" | "important" | "caution" | "admonition" => Block::Callout {
            kind: match node.kind.as_str() {
                "warning" => CalloutKind::Warning,
                "tip" => CalloutKind::Tip,
                "important" => CalloutKind::Important,
                "caution" => CalloutKind::Caution,
                _ => CalloutKind::Note,
            },
            attributes: Attributes::default(),
            blocks: children(warnings),
            span,
        },
        "index" | "target" | "comment" | "pending" | "docinfo" => return vec![],
        _ => {
            warnings.insert(format!("Unsupported Sphinx block node: {}", node.kind));
            Block::Unsupported {
                source_kind: node.kind.clone(),
                raw: node.text.clone(),
                span,
            }
        }
    }]
}
