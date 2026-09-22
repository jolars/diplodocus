//! Text conversion does not grant URL, asset, or HTML rendering trust.

use super::{AcceptedRepresentation, CandidateValidation, OutputCandidate};
use crate::diagnostics::DiagnosticCode;
use crate::documents::{MarkdownFragmentOrigin, parse_markdown_fragment};
use crate::execution::ExecutionFailure;
use crate::ir::OutputRepresentation;
use crate::provenance::fingerprint_bytes;
use serde_json::Value;

/// Validate text payloads and parse explicitly selected Markdown in isolation.
/// Asset and HTML validators compose at the same candidate boundary.
pub fn validate_text(
    candidate: OutputCandidate<'_>,
) -> Result<CandidateValidation, ExecutionFailure> {
    if !matches!(candidate.media_type, "text/plain" | "text/markdown") {
        return Ok(CandidateValidation {
            accepted: None,
            diagnostics: vec![candidate.warning(
                DiagnosticCode::UnsupportedCellOutput,
                "This output representation has no installed validator.",
            )],
        });
    }
    let text = match candidate.data {
        Value::String(text) => Some(text.clone()),
        Value::Array(lines) => lines
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<_>>>()
            .map(|lines| lines.concat()),
        _ => None,
    };
    let Some(text) = text else {
        return Ok(CandidateValidation {
            accepted: None,
            diagnostics: vec![candidate.warning(
                DiagnosticCode::InvalidCellOutput,
                if candidate.media_type == "text/markdown" {
                    "A Markdown payload must be a string or an array of strings."
                } else {
                    "A plain-text payload must be a string or an array of strings."
                },
            )],
        });
    };
    let content_fingerprint = fingerprint_bytes(text.as_bytes());
    let (representation, provenance, diagnostics) = if candidate.media_type == "text/markdown" {
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
        (
            parsed.representation,
            vec![parsed.provenance],
            parsed.diagnostics,
        )
    } else {
        (
            OutputRepresentation::PlainText {
                media_type: "text/plain".into(),
                text,
            },
            Vec::new(),
            Vec::new(),
        )
    };
    Ok(CandidateValidation {
        accepted: Some(AcceptedRepresentation {
            representation,
            content_fingerprint,
            policy: None,
            provenance,
        }),
        diagnostics,
    })
}
