//! Live text alternatives retain their validated content alongside portable IR.

use super::{AcceptedRepresentation, CandidateValidation, OutputCandidate};
use crate::documents::{MarkdownFragmentOrigin, parse_markdown_fragment};
use crate::execution::assets::PageAssetStore;
use crate::execution::output_safety::{
    ExecutionDiagnostic, FragmentIdentity, Validation, validate_html_live, validate_markdown_live,
};
use crate::execution::validated::{
    OwnedRepresentation, RepresentationContent, project_representation,
};
use crate::execution::{ExecutionFailure, ExecutionFailureKind};
use crate::ir::{OutputRepresentation, SanitizerProvenance, UnvalidatedHtml};
use crate::provenance::fingerprint_bytes;
use serde_json::Value;

pub(super) fn validate_text_with_assets(
    candidate: OutputCandidate<'_>,
    assets: &mut PageAssetStore,
) -> Result<CandidateValidation, ExecutionFailure> {
    let rejected = |diagnostic| {
        Ok(CandidateValidation {
            accepted: None,
            diagnostics: vec![diagnostic],
        })
    };
    if !matches!(
        candidate.media_type,
        "text/plain" | "text/markdown" | "text/html"
    ) {
        return rejected(ExecutionDiagnostic::UnsupportedMedia {
            attribution: candidate.attribution(),
            media_type: candidate.media_type.into(),
        });
    }
    let Some(text) = text_payload(candidate.data) else {
        return rejected(ExecutionDiagnostic::InvalidTextPayload {
            attribution: candidate.attribution(),
            media_type: candidate.media_type.into(),
        });
    };
    let mut origin = candidate.origin();
    if candidate.media_type == "text/plain" {
        return Ok(CandidateValidation {
            accepted: Some(AcceptedRepresentation {
                representation: OutputRepresentation::PlainText {
                    media_type: "text/plain".into(),
                    text: text.clone(),
                },
                content_fingerprint: fingerprint_bytes(text.as_bytes()),
                policy: None,
                provenance: vec![],
                value: OwnedRepresentation::Text(text),
            }),
            diagnostics: vec![],
        });
    }
    let (value, diagnostics) = if candidate.media_type == "text/markdown" {
        origin.fragment = Some(FragmentIdentity {
            ordinal: candidate.fragment_ordinal,
            byte_length: text.len(),
        });
        let mut source = candidate.page.source.clone();
        source.span = Some(candidate.cell.cell.span);
        let parsed = parse_markdown_fragment(
            &text,
            MarkdownFragmentOrigin {
                collection: candidate.page.collection.clone(),
                cell: candidate.cell.ordinal,
                output: candidate.slot,
                source,
            },
        );
        match validate_markdown_live(parsed, &origin, candidate.context, assets)? {
            Validation::Accepted { value, diagnostics } => (
                OwnedRepresentation::Markdown {
                    value: Box::new(value),
                    origin,
                },
                diagnostics,
            ),
            Validation::Rejected { diagnostics } => {
                return Ok(CandidateValidation {
                    accepted: None,
                    diagnostics,
                });
            }
        }
    } else {
        match validate_html_live(&text, &origin, candidate.context, assets)? {
            Validation::Accepted { value, diagnostics } => {
                (OwnedRepresentation::Html { value }, diagnostics)
            }
            Validation::Rejected { diagnostics } => {
                return Ok(CandidateValidation {
                    accepted: None,
                    diagnostics,
                });
            }
        }
    };
    let (representation, content, policy, provenance) = match &value {
        OwnedRepresentation::Markdown { value, .. } => (
            OutputRepresentation::MarkdownBlocks {
                media_type: "text/markdown".into(),
                blocks: value.blocks().to_vec(),
            },
            RepresentationContent::Markdown(value.canonical_content()),
            "qmd-mvp-v1",
            vec![value.provenance().clone()],
        ),
        OwnedRepresentation::Html { value } => (
            OutputRepresentation::HtmlCandidate {
                media_type: "text/html".into(),
                html: UnvalidatedHtml::new(value.canonical_content().markup.clone()),
                policy: "html-mvp-v1".into(),
                sanitizer: SanitizerProvenance {
                    name: "diplodocus-html-sanitizer".into(),
                    version: env!("CARGO_PKG_VERSION").into(),
                },
            },
            RepresentationContent::Html(value.canonical_content()),
            "html-mvp-v1",
            vec![],
        ),
        _ => unreachable!("Literal text returned before structural validation."),
    };
    let content_fingerprint = project_representation(content)
        .map_err(|_| {
            candidate.failure(
                ExecutionFailureKind::OutputValidation,
                "Generated content could not be fingerprinted.",
            )
        })?
        .fingerprint;
    Ok(CandidateValidation {
        accepted: Some(AcceptedRepresentation {
            representation,
            content_fingerprint,
            policy: Some(policy.into()),
            provenance,
            value,
        }),
        diagnostics,
    })
}

#[cfg(test)]
pub(in crate::execution::jupyter) fn validate_text(
    candidate: OutputCandidate<'_>,
) -> Result<CandidateValidation, ExecutionFailure> {
    let root = tempfile::tempdir().unwrap();
    let mut assets = PageAssetStore::new(
        candidate.page.clone(),
        root.path().into(),
        root.path().join("staging"),
    )
    .unwrap();
    validate_text_with_assets(candidate, &mut assets)
}

pub(super) fn text_payload(data: &Value) -> Option<String> {
    match data {
        Value::String(text) => Some(text.clone()),
        Value::Array(lines) => lines
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<_>>>()
            .map(|lines| lines.concat()),
        _ => None,
    }
}
