//! Inline image conversion at the same incremental boundary as text conversion.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use super::text::{text_payload, validate_text_with_assets};
use super::{AcceptedRepresentation, CandidateValidation, OutputCandidate};
use crate::execution::ExecutionFailure;
use crate::execution::assets::{AssetError, PageAssetStore};
use crate::execution::output_safety::ExecutionDiagnostic;
use crate::execution::validated::OwnedRepresentation;
use crate::ir::OutputRepresentation;

/// Validate every supported image alternative, including hidden output. Generated
/// Markdown images and HTML use the same page-scoped staging owner.
pub fn validate_with_assets(
    candidate: OutputCandidate<'_>,
    assets: &mut PageAssetStore,
) -> Result<CandidateValidation, ExecutionFailure> {
    if !matches!(
        candidate.media_type,
        "image/png" | "image/jpeg" | "image/svg+xml"
    ) {
        return validate_text_with_assets(candidate, assets);
    }
    let result = (|| {
        let payload = text_payload(candidate.data).ok_or(AssetError::InvalidMedia)?;
        let bytes = if candidate.media_type == "image/svg+xml" {
            payload.into_bytes()
        } else {
            STANDARD
                .decode(payload)
                .map_err(|_| AssetError::InvalidMedia)?
        };
        assets.stage_bytes(candidate.media_type, &bytes)
    })();
    match result {
        Ok(asset) => Ok(CandidateValidation {
            accepted: Some(AcceptedRepresentation {
                value: OwnedRepresentation::Asset(asset.clone()),
                content_fingerprint: asset.reference.fingerprint.clone(),
                representation: OutputRepresentation::Asset {
                    media_type: asset.media_type,
                    asset: asset.reference,
                },
                policy: Some(
                    if candidate.media_type == "image/svg+xml" {
                        "svg-mvp-v1"
                    } else {
                        "mime-mvp-v1"
                    }
                    .into(),
                ),
                provenance: Vec::new(),
            }),
            diagnostics: Vec::new(),
        }),
        Err(error) => match error.failure_kind() {
            Some(kind) => Err(candidate.failure(kind, &error.to_string())),
            None => Ok(CandidateValidation {
                accepted: None,
                diagnostics: vec![if error == AssetError::UnsafeSvg {
                    ExecutionDiagnostic::SvgRejected {
                        attribution: candidate.attribution(),
                    }
                } else {
                    ExecutionDiagnostic::InvalidImage {
                        attribution: candidate.attribution(),
                        media_type: candidate.media_type.into(),
                    }
                }],
            }),
        },
    }
}
