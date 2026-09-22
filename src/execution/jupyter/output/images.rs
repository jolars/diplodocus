//! Inline image conversion at the same incremental boundary as text conversion.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use super::text::{text_payload, validate_text};
use super::{AcceptedRepresentation, CandidateValidation, OutputCandidate};
use crate::execution::ExecutionFailure;
use crate::execution::assets::{AssetError, PageAssetStore};
use crate::ir::OutputRepresentation;

/// Validate every supported image alternative, including hidden output. Generated
/// Markdown images and HTML require their later structural safety validators.
pub fn validate_with_assets(
    candidate: OutputCandidate<'_>,
    assets: &mut PageAssetStore,
) -> Result<CandidateValidation, ExecutionFailure> {
    if !matches!(
        candidate.media_type,
        "image/png" | "image/jpeg" | "image/svg+xml"
    ) {
        return validate_text(candidate);
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
                diagnostics: vec![candidate.warning(error.diagnostic_code(), &error.to_string())],
            }),
        },
    }
}
