//! Native Documenter evidence must reproduce the retained example presentation.

use super::*;
use crate::documents::{AuthoredFormat, documenter};

pub(super) fn validate(snapshot: &Snapshot) -> Result<(), SnapshotError> {
    let workspace = &snapshot.workspace;
    for (page_id, page) in &workspace.pages {
        let records: Vec<_> = page
            .document
            .provenance
            .iter()
            .filter(|record| {
                matches!(
                    record.activity,
                    ProvenanceActivity::DocumenterExamples { .. }
                )
            })
            .collect();
        let declaration = match &page.kind {
            PageKind::Authored { collection } => workspace
                .content_collections
                .get(collection)
                .map(|value| (collection, value)),
            _ => None,
        };
        let Some((collection_id, collection)) =
            declaration.filter(|(_, collection)| collection.format == AuthoredFormat::Documenter)
        else {
            require(
                records.is_empty(),
                "Documenter evidence requires an authored Documenter page",
            )?;
            continue;
        };
        let config = crate::configuration::ContentConfiguration {
            id: collection_id.clone(),
            owner: "project".into(),
            repository: collection.repository.clone(),
            path: ".".into(),
            mount: collection.mount.clone(),
            format: collection.format,
            navigation: collection.navigation,
            execution: crate::configuration::ExecutionConfiguration {
                mode: collection.execution.mode,
                engine: collection.execution.engine,
                kernel: collection.execution.kernel.clone(),
                julia_project: collection
                    .execution
                    .julia_project
                    .as_ref()
                    .map(|path| path.as_str().into()),
                declared_environment_inputs: Vec::new(),
            },
        };
        config
            .validate_execution()
            .map_err(|_| SnapshotError::Invalid("Documenter execution configuration"))?;
        if collection.execution.mode == ExecutionMode::Never {
            require(records.is_empty(), "Disabled Documenter execution evidence")?;
            continue;
        }
        let raw = page
            .document
            .raw_source
            .as_ref()
            .ok_or(SnapshotError::Invalid("Documenter original source"))?;
        let prepared = documenter::examples(raw)
            .map_err(|_| SnapshotError::Invalid("Documenter execution preparation"))?;
        if prepared.is_empty() {
            require(records.is_empty(), "Empty Documenter execution evidence")?;
            continue;
        }
        require(
            records.len() == 1,
            "Documenter requires completed native execution",
        )?;
        let ProvenanceActivity::DocumenterExamples {
            collection: observed_collection,
            input,
            environment_inputs,
            examples,
            ..
        } = &records[0].activity
        else {
            unreachable!()
        };
        require(
            observed_collection == collection_id
                && page.document.source_location.as_ref() == Some(&input.source)
                && input.fingerprint == fingerprint_bytes(raw.as_bytes())
                && examples.len() == prepared.len()
                && environment_inputs.iter().all(|input| {
                    input.source.repository == collection.repository
                        && valid_fingerprint(&input.fingerprint)
                })
                && environment_inputs
                    .windows(2)
                    .all(|pair| pair[0].source.path < pair[1].source.path)
                && environment_inputs
                    .iter()
                    .map(|input| input.source.path.clone())
                    .collect::<BTreeSet<_>>()
                    == collection.execution.declared_environment_inputs,
            "Documenter execution source association",
        )?;
        let mut expected = Vec::new();
        for (example, outcome) in prepared.iter().zip(examples) {
            require(
                example.span == outcome.span && example.session == outcome.session,
                "Documenter example source association",
            )?;
            for output in &outcome.outputs {
                if let DocumenterOutput::Image { media_type, asset } = output {
                    let extension = match media_type.as_str() {
                        "image/png" => "png",
                        "image/jpeg" => "jpg",
                        "image/svg+xml" => "svg",
                        _ => return Err(SnapshotError::Invalid("Documenter image media type")),
                    };
                    require(
                        asset.fingerprint.algorithm == "sha256"
                            && asset.path.as_str()
                                == format!(
                                    "documenter-assets/sha256/{}.{extension}",
                                    asset.fingerprint.value
                                ),
                        "Documenter image identity",
                    )?;
                    let stored = snapshot
                        .assets
                        .get(&asset.fingerprint.value)
                        .ok_or(SnapshotError::Invalid("Missing Documenter image"))?;
                    require(
                        stored.media_type == *media_type && stored.fingerprint == asset.fingerprint,
                        "Documenter image media association",
                    )?;
                    let resolved = snapshot
                        .documents
                        .iter()
                        .find(|document| {
                            document.document
                                == (DocumentIdentity::Page {
                                    page: page_id.clone(),
                                })
                        })
                        .ok_or(SnapshotError::Invalid("Documenter resolved page"))?;
                    require(resolved.references.iter().any(|reference| reference.kind == ReferenceKind::Image
                        && reference.spelling == asset.path.as_str()
                        && matches!(&reference.target, ReferenceTarget::Asset { asset: target, fragment: None }
                            if target.fingerprint == asset.fingerprint && target.path.as_str() == format!("content-assets/sha256/{}", asset.fingerprint.value))), "Documenter image resolution association")?;
                }
            }
            expected.push(documenter::rendered(example, outcome));
        }
        let mut actual = Vec::new();
        let mut blocks = page.document.document.blocks.clone();
        documenter::rewrite(&mut blocks, &mut |block| {
            if matches!(
                block,
                Block::Container {
                    kind: DocumentContainerKind::GeneratedChunk,
                    ..
                }
            ) {
                actual.push(block.clone());
            }
        });
        require(
            actual == expected,
            "Documenter execution output association",
        )?;
    }
    Ok(())
}

fn valid_fingerprint(fingerprint: &Fingerprint) -> bool {
    fingerprint.algorithm == "sha256"
        && fingerprint.value.len() == 64
        && fingerprint
            .value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
