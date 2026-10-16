use super::*;
use crate::diagnostics::DiagnosticEntity;
use crate::extractors::{julia, python, r};
use crate::ir::{ExtractionTarget, Item, Package, Provenance, ProvenanceActivity, SourceLocation};

pub(super) fn assemble(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    workspace: &mut Workspace,
    selections: &mut DeclaredSourceInputs,
    observed: &mut ObservedInputs,
    julia_results: &mut BTreeMap<(String, String), julia::JuliaExtraction>,
    rust_results: &mut BTreeMap<(String, String), super::rust_native::RustNativeExtraction>,
) -> Result<(), AssemblyError> {
    for (index, declaration) in configuration.packages.iter().enumerate() {
        let resolved = &paths.packages[index];
        let repository = &paths.repositories[resolved.repository_index];
        let mut package = Package {
            slug: declaration.slug.clone(),
            name: declaration.name.clone(),
            ecosystem: declaration.ecosystem.clone(),
            version: None,
            metadata: None,
            repository: declaration.repository.clone(),
            path: relative(&resolved.path, &repository.path)?,
            metadata_path: relative(&resolved.metadata_path, &resolved.path)?
                .ok_or(AssemblyError::InputsChanged)?,
            kind: declaration.kind,
            visibility: declaration.visibility,
            extraction_targets: BTreeMap::new(),
            items: BTreeMap::new(),
        };
        for (target, selected) in declaration.targets.iter().zip(&resolved.targets) {
            package.extraction_targets.insert(
                target.id.clone(),
                ExtractionTarget {
                    extractor: target.extractor.clone(),
                    path: relative(&selected.path, &resolved.path)?,
                    role: target.role.clone(),
                },
            );
            let fragment = match target.extractor.as_str() {
                "rust" => {
                    let result = rust_results
                        .remove(&(declaration.id.clone(), target.id.clone()))
                        .ok_or(AssemblyError::InputsChanged)?;
                    if let Some(doctests) = result.doctests {
                        workspace.provenance.push(doctests);
                    }
                    Fragment {
                        metadata: None,
                        version: result.extraction.version,
                        items: result.extraction.items,
                        diagnostics: result.extraction.diagnostics,
                        provenance: result.provenance,
                    }
                }
                "julia" => {
                    let result = julia_results
                        .remove(&(declaration.id.clone(), target.id.clone()))
                        .unwrap_or_else(|| julia::extract_target(repository, resolved, selected));
                    Fragment {
                        metadata: None,
                        version: result.metadata.and_then(|metadata| metadata.version),
                        items: result.items,
                        diagnostics: result.diagnostics,
                        provenance: result.provenance,
                    }
                }
                "python" => {
                    let result = python::extract_target(repository, resolved, selected);
                    Fragment {
                        metadata: None,
                        version: result.metadata.map(|m| m.version),
                        items: result.items,
                        diagnostics: result.diagnostics,
                        provenance: result.provenance,
                    }
                }
                "r" => {
                    let result = r::extract_target(repository, resolved, selected);
                    Fragment {
                        version: result.metadata.as_ref().map(|m| m.version.clone()),
                        metadata: result.metadata.map(|description| {
                            crate::ir::PackageMetadata::R(crate::ir::RPackageMetadata {
                                description,
                                help_links: result.help_links,
                                runtime: None,
                            })
                        }),
                        items: result.items,
                        diagnostics: result.diagnostics,
                        provenance: result.provenance,
                    }
                }
                _ => {
                    workspace.diagnostics.insert(
                        diagnostic(
                            DiagnosticCode::UnsupportedExtractor,
                            format!(
                                "No built-in adapter implements extractor `{}`.",
                                target.extractor
                            ),
                        )
                        .with_entity(DiagnosticEntity::Target {
                            package: declaration.id.clone(),
                            id: target.id.clone(),
                        }),
                    );
                    continue;
                }
            };
            workspace.diagnostics.extend(fragment.diagnostics);
            if let Some(mut metadata) = fragment.metadata {
                if let (
                    Some(crate::ir::PackageMetadata::R(previous)),
                    crate::ir::PackageMetadata::R(current),
                ) = (&package.metadata, &mut metadata)
                {
                    for (spelling, targets) in &previous.help_links {
                        current
                            .help_links
                            .entry(spelling.clone())
                            .or_default()
                            .extend(targets.iter().cloned());
                    }
                    for targets in current.help_links.values_mut() {
                        targets.sort();
                        targets.dedup();
                    }
                }
                if package
                    .metadata
                    .as_ref()
                    .is_some_and(|previous| match (previous, &metadata) {
                        (
                            crate::ir::PackageMetadata::R(previous),
                            crate::ir::PackageMetadata::R(current),
                        ) => {
                            previous.description != current.description
                                || previous.runtime != current.runtime
                        }
                    })
                {
                    workspace.diagnostics.insert(
                        diagnostic(
                            DiagnosticCode::ConflictingExtraction,
                            "Extraction targets observed different package metadata.",
                        )
                        .with_entity(DiagnosticEntity::Package {
                            id: declaration.id.clone(),
                        }),
                    );
                } else {
                    package.metadata = Some(metadata);
                }
            }
            if package.version.is_some()
                && fragment.version.is_some()
                && package.version != fragment.version
            {
                workspace.diagnostics.insert(
                    diagnostic(
                        DiagnosticCode::ConflictingExtraction,
                        "Extraction targets observed different package versions.",
                    )
                    .with_entity(DiagnosticEntity::Package {
                        id: declaration.id.clone(),
                    }),
                );
            } else if package.version.is_none() {
                package.version = fragment.version;
            }
            for (id, item) in fragment.items {
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    package.items.entry(id.clone())
                {
                    entry.insert(item);
                } else {
                    workspace.diagnostics.insert(
                        diagnostic(
                            DiagnosticCode::ConflictingExtraction,
                            format!("Multiple extraction targets claim item `{id}`."),
                        )
                        .with_entity(DiagnosticEntity::Item {
                            package: declaration.id.clone(),
                            id,
                        }),
                    );
                }
            }
            if let ProvenanceActivity::Extraction {
                target: reference,
                inputs,
                ..
            } = &fragment.provenance.activity
            {
                let mut selected_inputs = vec![];
                for (repository_id, inputs) in inputs {
                    if repository_id != &repository.id {
                        return Err(AssemblyError::InputsChanged);
                    }
                    for (path, input) in inputs {
                        let location = SourceLocation {
                            repository: repository_id.clone(),
                            path: path.clone(),
                            span: None,
                        };
                        observe(observed, &location, input.fingerprint.clone())?;
                        let absolute = repository.path.join(path.as_str());
                        if let Ok(relative) = absolute.strip_prefix(&resolved.path) {
                            selected_inputs.push(relative.to_owned());
                        } else if target.extractor == "rust" {
                            selections
                                .package_documentation
                                .entry(repository.id.clone())
                                .or_default()
                                .push(PathBuf::from(path.as_str()));
                        } else {
                            return Err(AssemblyError::InputsChanged);
                        }
                    }
                }
                selections
                    .extraction
                    .insert(reference.clone(), selected_inputs);
            }
            workspace.provenance.push(fragment.provenance);
        }
        workspace.packages.insert(declaration.id.clone(), package);
    }
    Ok(())
}

struct Fragment {
    metadata: Option<crate::ir::PackageMetadata>,
    version: Option<String>,
    items: BTreeMap<String, Item>,
    diagnostics: Vec<Diagnostic>,
    provenance: Provenance,
}
