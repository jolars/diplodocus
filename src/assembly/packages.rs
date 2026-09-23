use super::*;
use crate::diagnostics::DiagnosticEntity;
use crate::extractors::{python, r};
use crate::ir::{ExtractionTarget, Item, Package, Provenance, ProvenanceActivity, SourceLocation};

pub(super) fn assemble(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    workspace: &mut Workspace,
    selections: &mut DeclaredSourceInputs,
    observed: &mut ObservedInputs,
) -> Result<(), AssemblyError> {
    for (index, declaration) in configuration.packages.iter().enumerate() {
        let resolved = &paths.packages[index];
        let repository = &paths.repositories[resolved.repository_index];
        let mut package = Package {
            slug: declaration.slug.clone(),
            name: declaration.name.clone(),
            ecosystem: declaration.ecosystem.clone(),
            version: None,
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
                "python" => {
                    let result = python::extract_target(repository, resolved, selected);
                    Fragment {
                        version: result.metadata.map(|m| m.version),
                        items: result.items,
                        diagnostics: result.diagnostics,
                        provenance: result.provenance,
                    }
                }
                "r" => {
                    let result = r::extract_target(repository, resolved, selected);
                    Fragment {
                        version: result.metadata.map(|m| m.version),
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
            if let ProvenanceActivity::Extraction { target, inputs, .. } =
                &fragment.provenance.activity
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
                        selected_inputs.push(
                            repository
                                .path
                                .join(path.as_str())
                                .strip_prefix(&resolved.path)
                                .map_err(|_| AssemblyError::InputsChanged)?
                                .to_owned(),
                        );
                    }
                }
                selections
                    .extraction
                    .insert(target.clone(), selected_inputs);
            }
            workspace.provenance.push(fragment.provenance);
        }
        workspace.packages.insert(declaration.id.clone(), package);
    }
    Ok(())
}

struct Fragment {
    version: Option<String>,
    items: BTreeMap<String, Item>,
    diagnostics: Vec<Diagnostic>,
    provenance: Provenance,
}
