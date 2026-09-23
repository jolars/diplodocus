use super::*;
use crate::diagnostics::DiagnosticEntity;
use crate::ir::{Concept, PackageReference, PackageRelationship, Provenance, ProvenanceActivity};
use crate::provenance::builtin_tools;
use crate::validation::resolve_package_item;
use std::collections::BTreeSet;

pub(super) fn concepts_and_relationships(
    configuration: &WorkspaceConfiguration,
    workspace: &mut Workspace,
    config_name: &DiagnosticPath,
) {
    for concept in &configuration.concepts {
        let mut members = BTreeSet::new();
        for member in &concept.members {
            match resolve_package_item(workspace, &member.package, &member.item) {
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
