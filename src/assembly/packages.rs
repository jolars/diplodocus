use super::*;
use crate::diagnostics::DiagnosticEntity;
use crate::extractors::adapter::AdapterOutput;
use crate::extractors::python::native::DoctestPlans;
use crate::ir::{
    ExtractionTarget, Item, ItemLanguageData, Package, PythonVisibility, SourceLocation,
};
use std::collections::{BTreeSet, btree_map::Entry};

pub(super) fn assemble(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    workspace: &mut Workspace,
    selections: &mut DeclaredSourceInputs,
    observed: &mut ObservedInputs,
    native: &mut super::NativeExtractions,
    reporting: &Reporting,
) -> Result<BTreeMap<String, DoctestPlans>, AssemblyError> {
    let total = configuration
        .packages
        .iter()
        .map(|package| package.targets.len())
        .sum();
    let mut progress = reporting.stage("Assembling API documentation", Some(total));
    let mut python_doctests = BTreeMap::new();
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
        let mut python_support = BTreeSet::new();
        let mut doctests = DoctestPlans::new();
        for (target, selected) in declaration.targets.iter().zip(&resolved.targets) {
            progress.item(&format!(
                "{}:{} ({})",
                declaration.id, target.id, target.extractor
            ));
            package.extraction_targets.insert(
                target.id.clone(),
                ExtractionTarget {
                    extractor: target.extractor.clone(),
                    path: relative(&selected.path, &resolved.path)?,
                    role: target.role.clone(),
                },
            );
            let mut support_items = BTreeSet::new();
            let mut target_doctests = DoctestPlans::new();
            let fragment: AdapterOutput = match target.extractor.as_str() {
                "doxygen" => {
                    let result = native
                        .doxygen
                        .remove(&(declaration.id.clone(), target.id.clone()))
                        .ok_or(AssemblyError::InputsChanged)?;
                    result.into()
                }
                "rust" => native
                    .rust
                    .remove(&(declaration.id.clone(), target.id.clone()))
                    .ok_or(AssemblyError::InputsChanged)?
                    .into(),
                "julia" => {
                    let result = native
                        .julia
                        .remove(&(declaration.id.clone(), target.id.clone()))
                        .ok_or(AssemblyError::InputsChanged)?;
                    let mut fragment = AdapterOutput::from(result.extraction);
                    fragment.evidence.completed.push(result.completed);
                    fragment
                }
                "python" => {
                    let result = native
                        .python
                        .remove(&(declaration.id.clone(), target.id.clone()))
                        .ok_or(AssemblyError::InputsChanged)?;
                    support_items = result.support_items;
                    target_doctests = result.doctests;
                    let mut fragment = AdapterOutput::from(result.extraction);
                    fragment.evidence.completed.push(result.completed);
                    fragment
                }
                "r" => {
                    let result = native
                        .r
                        .remove(&(declaration.id.clone(), target.id.clone()))
                        .ok_or(AssemblyError::InputsChanged)?;
                    let mut fragment = AdapterOutput::from(result.extraction);
                    fragment.evidence.completed = result.completed;
                    fragment
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
            let reference = fragment.evidence.target();
            if reference.package != declaration.id || reference.target != target.id {
                return Err(AssemblyError::InputsChanged);
            }
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
                        _ => previous != &metadata,
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
                let support = support_items.contains(&id);
                let incoming_doc = match package.items.entry(id.clone()) {
                    Entry::Vacant(entry) => {
                        if support {
                            python_support.insert(id.clone());
                        }
                        entry.insert(item);
                        true
                    }
                    Entry::Occupied(mut entry) => {
                        if let Some(incoming_doc) = merge_python_support(
                            entry.get_mut(),
                            item,
                            python_support.contains(&id),
                            support,
                        ) {
                            if !support {
                                python_support.remove(&id);
                            }
                            incoming_doc
                        } else {
                            workspace.diagnostics.insert(
                                diagnostic(
                                    DiagnosticCode::ConflictingExtraction,
                                    format!("Multiple extraction targets claim item `{id}`."),
                                )
                                .with_entity(
                                    DiagnosticEntity::Item {
                                        package: declaration.id.clone(),
                                        id: id.clone(),
                                    },
                                ),
                            );
                            false
                        }
                    }
                };
                if incoming_doc {
                    if let Some(plan) = target_doctests.remove(&id) {
                        doctests.insert(id, plan);
                    } else {
                        doctests.remove(&id);
                    }
                }
            }
            {
                let reference = fragment.evidence.target();
                let inputs = fragment.evidence.inputs();
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
                        } else if matches!(target.extractor.as_str(), "rust" | "doxygen") {
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
            workspace.provenance.extend(fragment.evidence.completed);
            workspace.provenance.push(fragment.evidence.extraction);
            progress.advance();
        }
        if !doctests.is_empty() {
            python_doctests.insert(declaration.id.clone(), doctests);
        }
        workspace.packages.insert(declaration.id.clone(), package);
    }
    progress.finish();
    Ok(python_doctests)
}

/// Merge matching Python items and report whether the incoming item supplies the prose.
fn merge_python_support(
    previous: &mut Item,
    mut item: Item,
    previous_support: bool,
    support: bool,
) -> Option<bool> {
    if !(previous_support || support) {
        return None;
    }
    let left = previous.source_location.as_ref()?;
    let right = item.source_location.as_ref()?;
    if left.repository != right.repository || left.path != right.path {
        return None;
    }
    let (Some(ItemLanguageData::Python(existing)), Some(ItemLanguageData::Python(incoming))) =
        (&mut previous.language_data, &mut item.language_data)
    else {
        return None;
    };
    // Keep definition facts from the selecting target. Support items can
    // contribute public names, visibility, and edges from the reexport.
    let prefer_incoming = previous_support && !support;
    let incoming_doc =
        previous.documentation.is_none() || (prefer_incoming && item.documentation.is_some());
    let (selected, other) = if prefer_incoming {
        (incoming, existing)
    } else {
        (existing, incoming)
    };
    if other.visibility == PythonVisibility::Public
        || selected.visibility == PythonVisibility::Private
    {
        selected.visibility = other.visibility;
    }
    if prefer_incoming {
        std::mem::swap(previous, &mut item);
    }
    if previous.documentation.is_none() {
        previous.documentation = item.documentation;
    }
    for child in item.children {
        if !previous.children.contains(&child) {
            previous.children.push(child);
        }
    }
    for alias in item.aliases {
        if let Some(existing) = previous.aliases.iter_mut().find(|existing| {
            existing.qualified_name == alias.qualified_name && existing.kind == alias.kind
        }) {
            for source in alias.sources {
                if !existing.sources.contains(&source) {
                    existing.sources.push(source);
                }
            }
        } else {
            previous.aliases.push(alias);
        }
    }
    previous
        .aliases
        .sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
    Some(incoming_doc)
}

pub(super) fn evidence_current(
    provenance: &crate::ir::Provenance,
    repository: &crate::paths::ResolvedRepositoryPaths,
) -> bool {
    let crate::ir::ProvenanceActivity::Extraction { inputs, .. } = &provenance.activity else {
        return false;
    };
    inputs.iter().all(|(id, inputs)| {
        id == &repository.id
            && inputs.iter().all(|(path, input)| {
                fs::read(repository.path.join(path.as_str()))
                    .is_ok_and(|bytes| fingerprint_bytes(&bytes) == input.fingerprint)
            })
    })
}
