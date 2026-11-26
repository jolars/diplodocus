//! Verify that native guide results reproduce the published document.

use super::*;
use crate::documents::{ParseMode, guide_examples, includes, parse_document_with_resources};

pub(super) fn validate(snapshot: &Snapshot) -> Result<(), SnapshotError> {
    let workspace = &snapshot.workspace;
    for (id, page) in &workspace.pages {
        let records: Vec<_> = page
            .document
            .provenance
            .iter()
            .filter_map(|record| match &record.activity {
                ProvenanceActivity::GuideExamples { input, examples } => Some((input, examples)),
                _ => None,
            })
            .collect();
        let collection = match &page.kind {
            PageKind::Authored { collection } => workspace.content_collections.get(collection),
            _ => None,
        };
        let Some(collection) = collection.filter(|collection| guide_examples::enabled(collection))
        else {
            require(records.is_empty(), "unexpected guide execution evidence")?;
            continue;
        };

        let raw = page
            .document
            .raw_source
            .as_ref()
            .ok_or(SnapshotError::Invalid("missing guide source"))?;
        let plan = guide_examples::plan(raw, collection, workspace, |id| {
            // Only the selected checkouts are needed to verify persisted execution.
            records
                .first()
                .is_none_or(|(_, results)| results.iter().any(|result| result.package == id))
        })
        .map_err(|_| SnapshotError::Invalid("invalid guide execution selection"))?;
        if plan.is_empty() {
            require(records.is_empty(), "unexpected guide execution evidence")?;
            continue;
        }
        let [(input, examples)] = records.as_slice() else {
            return Err(SnapshotError::Invalid(
                "missing or duplicate guide execution evidence",
            ));
        };
        require(
            page.document.source_location.as_ref() == Some(&input.source)
                && input.fingerprint == fingerprint_bytes(raw.as_bytes())
                && plan.len() == examples.len(),
            "guide execution input association",
        )?;
        let resolved = snapshot
            .documents
            .iter()
            .find(|record| record.document == (DocumentIdentity::Page { page: id.clone() }))
            .ok_or(SnapshotError::Invalid("missing resolved guide"))?;
        for (block, result) in plan.iter().zip(*examples) {
            require(
                block.span == result.span
                    && block.package == result.package
                    && !result.runtime_version.trim().is_empty(),
                "guide execution result association",
            )?;
            for output in &result.outputs {
                if let DocumenterOutput::Image { asset, media_type } = output {
                    let stored = snapshot
                        .assets
                        .get(&asset.fingerprint.value)
                        .ok_or(SnapshotError::Invalid("missing guide image"))?;
                    require(
                        block.language == "r"
                            && media_type == "image/png"
                            && stored.media_type == *media_type
                            && stored.fingerprint == asset.fingerprint
                            && asset.path.as_str()
                                == format!("guide-assets/sha256/{}.png", asset.fingerprint.value),
                        "guide image association",
                    )?;
                    require(
                        resolved.references.iter().any(|reference| {
                            reference.kind == ReferenceKind::Image
                                && reference.spelling == asset.path.as_str()
                                && matches!(&reference.target, ReferenceTarget::Asset { asset: target, fragment: None }
                                    if target.fingerprint == asset.fingerprint
                                        && target.path.as_str() == format!("content-assets/sha256/{}", asset.fingerprint.value))
                        }),
                        "guide image resolution association",
                    )?;
                }
            }
        }
        let mut document = parse_document_with_resources(
            raw,
            collection.format,
            ParseMode::Collection,
            &|target, span| {
                includes::resource_target(target, span, &input.source, &resolved.source_ranges)
                    .unwrap_or_else(|| target.to_owned())
            },
        )
        .document;
        require(
            guide_examples::project(&mut document.blocks, examples)
                && document == page.document.document,
            "guide execution document projection",
        )?;
    }
    Ok(())
}
