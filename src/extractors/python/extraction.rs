//! Compose source observations, canonical public items, and inert documentation.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, DiagnosticEntity};
use crate::ir::{
    Item, ParserProvenance, Provenance, ProvenanceActivity, SchemaVersion, TargetReference,
};
use crate::paths::{ResolvedPackagePaths, ResolvedRepositoryPaths, ResolvedTargetPath};
use crate::provenance::PANACHE_VERSION;

use super::{PythonMetadata, docstrings, parse_target, surface};

/// Portable result of one Python target, ready for workspace fragment merging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonExtraction {
    /// Version of the shared portable IR vocabulary.
    pub schema_version: SchemaVersion,
    /// Workspace package whose canonical item IDs scope this fragment.
    pub package: String,
    /// Explicit extraction target.
    pub target: TargetReference,
    /// Static distribution metadata, absent when invalid or dynamic.
    pub metadata: Option<PythonMetadata>,
    /// Canonical API items with structured signatures and documentation.
    pub items: BTreeMap<String, Item>,
    /// Source, semantic, and documentation diagnostics in deterministic order.
    pub diagnostics: Vec<Diagnostic>,
    /// Actual parser observations and fingerprints for the complete extraction.
    pub provenance: Provenance,
}

/// Statically extract one configured Python target without starting a runtime.
///
/// Maintained stubs supply the public signatures while implementation docstrings
/// retain their own original source evidence. Documentation examples remain
/// inert. Errors do not prevent inspecting independent inputs, and callers must
/// reject error-bearing fragments before building a site. Workspace merging,
/// cross-package references, and routes belong to later pipeline stages.
pub fn extract_target(
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
    target: &ResolvedTargetPath,
) -> PythonExtraction {
    let parsed = parse_target(repository, package, target);
    let mut surface = surface::reconcile(&parsed);
    let mut provenance = parsed.provenance;
    let mut documentation_inputs = BTreeMap::<_, BTreeSet<String>>::new();
    for (id, docstring) in surface.docstrings {
        let source_map: Vec<_> = docstring
            .segments
            .into_iter()
            .map(|segment| docstrings::DocstringSourceSegment {
                decoded: segment.decoded,
                source: segment.source,
            })
            .collect();
        let input_key = (
            docstring.source.repository.clone(),
            docstring.source.path.clone(),
        );
        let mut document =
            docstrings::parse_docstring(&docstring.text, docstring.source, Some(&source_map));
        documentation_inputs.entry(input_key).or_default().extend(
            document
                .document
                .provenance
                .iter()
                .flat_map(|evidence| evidence.tools.keys())
                .filter(|name| matches!(name.as_str(), "pydocstring" | "panache-parser"))
                .cloned(),
        );
        for diagnostic in &mut document.diagnostics {
            diagnostic.related_entity = Some(DiagnosticEntity::Item {
                package: parsed.package.clone(),
                id: id.clone(),
            });
        }
        surface.diagnostics.extend(document.diagnostics);
        if let Some(item) = surface.items.get_mut(&id) {
            item.documentation = Some(document.document);
        }
    }
    if let ProvenanceActivity::Extraction {
        capabilities,
        parsers,
        inputs,
        ..
    } = &mut provenance.activity
    {
        capabilities.insert("diagnostics.unsupported-visible".into());
        if parsed.modules.iter().any(|module| module.valid) {
            capabilities.extend(
                [
                    "python.declarations",
                    "python.exports.static",
                    "python.overloads",
                    "python.reexports",
                    "python.stubs",
                ]
                .into_iter()
                .map(str::to_owned),
            );
        }
        if !documentation_inputs.is_empty() {
            capabilities.insert("python.docs.numpy".into());
            for (name, version, role, settings) in [
                (
                    "pydocstring",
                    "0.4.1",
                    "python-docstring",
                    BTreeMap::from([("style".into(), "numpy".into())]),
                ),
                (
                    "panache-parser",
                    PANACHE_VERSION,
                    "docstring-inline",
                    BTreeMap::from([
                        ("profile".into(), "gfm".into()),
                        ("execution".into(), "never".into()),
                    ]),
                ),
            ] {
                if !documentation_inputs
                    .values()
                    .any(|used| used.contains(name))
                {
                    continue;
                }
                parsers.insert(
                    name.into(),
                    ParserProvenance {
                        version: version.into(),
                        role: role.into(),
                        settings,
                    },
                );
                provenance.tools.insert(name.into(), version.into());
            }
            for ((repository, path), used) in documentation_inputs {
                if let Some(input) = inputs
                    .get_mut(&repository)
                    .and_then(|files| files.get_mut(&path))
                {
                    input.parsers.extend(used);
                }
            }
        }
    }
    surface.diagnostics.sort();
    surface.diagnostics.dedup();
    PythonExtraction {
        schema_version: SchemaVersion,
        package: parsed.package,
        target: parsed.target,
        metadata: parsed.metadata,
        items: surface.items,
        diagnostics: surface.diagnostics,
        provenance,
    }
}
