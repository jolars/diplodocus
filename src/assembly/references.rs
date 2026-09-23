use super::*;
use crate::diagnostics::DiagnosticEntity;
use crate::ir::{
    Concept, ItemLanguageData, ItemReference, PackageReference, PackageRelationship, Provenance,
    ProvenanceActivity, PythonCallableRole, PythonDeclaration,
};
use crate::provenance::builtin_tools;
use std::collections::BTreeSet;

pub(super) fn concepts_and_relationships(
    configuration: &WorkspaceConfiguration,
    workspace: &mut Workspace,
    config_name: &DiagnosticPath,
) {
    for concept in &configuration.concepts {
        let mut members = BTreeSet::new();
        for member in &concept.members {
            match resolve(workspace, &member.package, &member.item) {
                Ok(item) => {
                    members.insert(item);
                }
                Err(code) => {
                    workspace.diagnostics.insert(
                        diagnostic(
                            code,
                            format!(
                                "Concept member `{}::{}` does not identify exactly one item.",
                                member.package, member.item
                            ),
                        )
                        .with_entity(DiagnosticEntity::Concept {
                            id: concept.id.clone(),
                        })
                        .with_source(DiagnosticSource::Configuration {
                            path: config_name.clone(),
                        }),
                    );
                }
            }
        }
        workspace.concepts.insert(
            concept.id.clone(),
            Concept {
                kind: concept.kind,
                members,
                documentation: None,
            },
        );
    }
    for relationship in &configuration.relationships {
        let endpoint = |name: &str| {
            if workspace.packages.contains_key(name) {
                PackageReference::Workspace {
                    package: name.into(),
                }
            } else {
                let (ecosystem, name) = name
                    .split_once(':')
                    .expect("configuration-validated external coordinate");
                PackageReference::External {
                    ecosystem: ecosystem.into(),
                    name: name.into(),
                }
            }
        };
        workspace.relationships.push(PackageRelationship {
            from: endpoint(&relationship.from),
            to: endpoint(&relationship.to),
            kind: relationship.kind,
            version_constraint: relationship.version_constraint.clone(),
            provenance: vec![Provenance {
                activity: ProvenanceActivity::Declaration,
                source: Some(DiagnosticSource::Configuration {
                    path: config_name.clone(),
                }),
                span: None,
                tools: builtin_tools(),
            }],
        });
    }
}

fn resolve(
    workspace: &Workspace,
    package: &str,
    name: &str,
) -> Result<ItemReference, DiagnosticCode> {
    let Some(items) = workspace
        .packages
        .get(package)
        .map(|package| &package.items)
    else {
        return Err(DiagnosticCode::UnresolvedItemReference);
    };
    if items.contains_key(name) {
        return Ok(ItemReference {
            package: package.into(),
            item: name.into(),
        });
    }
    let candidates: Vec<_> = items.iter().filter(|(_, item)| {
        // Callable-family lookup must not become ambiguous with its overloads.
        !matches!(&item.language_data, Some(ItemLanguageData::Python(data)) if matches!(&data.declaration,
            PythonDeclaration::Callable { role: PythonCallableRole::Overload { .. }, .. }))
        && (item.qualified_name == name || item.aliases.iter().any(|alias| alias.qualified_name == name))
    }).collect();
    match candidates.as_slice() {
        [(id, _)] => Ok(ItemReference {
            package: package.into(),
            item: (*id).clone(),
        }),
        [] => Err(DiagnosticCode::UnresolvedItemReference),
        _ => Err(DiagnosticCode::AmbiguousItemReference),
    }
}
