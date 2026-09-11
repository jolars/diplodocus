//! Validation and semantic reference resolution.

use crate::configuration::{ContentConfiguration, ExecutionConfigurationError, ExecutionMode};
use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::documents::AuthoredFormat;
use crate::ir::{Document, MetadataEntry, MetadataValue, SourceSpan};

/// Validate a parsed document's execution declarations against its collection.
///
/// Collection authority is immutable: `execute: false` may restrict a page, and
/// cell defaults never enable a `never` collection. Document engine, kernel, and
/// mode selectors are rejected even if they match an authorized collection.
/// Unauthorized declarations share one diagnostic with related source ranges.
///
/// This checks execution declarations only; it does not normalize page or cell
/// options, validate other metadata, or select executable cells. It performs no
/// I/O and leaves the document and collection unchanged. The document must have
/// been parsed using the collection's authored format.
///
/// # Errors
///
/// Returns an error for an invalid collection execution configuration. Document
/// errors are returned as diagnostics in source order.
pub fn validate_document_execution(
    document: &Document,
    collection: &ContentConfiguration,
) -> Result<Vec<Diagnostic>, ExecutionConfigurationError> {
    collection.validate_execution()?;
    if collection.format == AuthoredFormat::Gfm {
        return Ok(Vec::new());
    }
    let Some(metadata) = &document.frontmatter else {
        return Ok(Vec::new());
    };
    let MetadataValue::Mapping { entries, .. } = metadata else {
        let span = match metadata {
            MetadataValue::Null { span }
            | MetadataValue::Scalar { span, .. }
            | MetadataValue::Sequence { span, .. }
            | MetadataValue::Unsupported { span, .. }
            | MetadataValue::Mapping { span, .. } => *span,
        };
        return Ok(vec![metadata_error(
            DiagnosticCode::InvalidQmdMetadata,
            "document metadata must be a mapping".to_owned(),
            span,
        )]);
    };

    let mut context = ExecutionAuthority {
        authorized: collection.execution.mode == ExecutionMode::Execute,
        unauthorized: Vec::new(),
        diagnostics: Vec::new(),
    };
    // Inspect retained declarations rather than selecting a winner: a duplicate
    // or later restriction must not hide an earlier attempt to grant authority.
    for entry in entries {
        match entry.key.value.as_str() {
            "jupyter" | "engine" | "kernel" | "execution" => {
                context.selector(entry, &entry.key.value);
            }
            "execute" => match &entry.value {
                MetadataValue::Scalar { raw, .. } if raw.trim() == "true" => {
                    if !context.authorized {
                        context.unauthorized.push(entry.span);
                    }
                }
                MetadataValue::Scalar { raw, .. } if raw.trim() == "false" => {}
                MetadataValue::Mapping { entries, .. } => {
                    for option in entries {
                        if matches!(option.key.value.as_str(), "mode" | "engine" | "kernel") {
                            context.selector(option, &format!("execute.{}", option.key.value));
                        }
                    }
                }
                _ => context.diagnostics.push(metadata_error(
                    DiagnosticCode::InvalidQmdMetadata,
                    "document `execute` must be an unquoted `true`, `false`, or a mapping"
                        .to_owned(),
                    entry.span,
                )),
            },
            _ => {}
        }
    }

    context
        .unauthorized
        .sort_by_key(|span| (span.start, span.end));
    if let Some((&span, related_spans)) = context.unauthorized.split_first() {
        context.diagnostics.push(Diagnostic {
            code: DiagnosticCode::DocumentExecutionNotAuthorized,
            severity: Severity::Error,
            message: "document metadata cannot authorize execution in a collection with `mode = \"never\"`"
                .to_owned(),
            span: Some(span),
            related_spans: related_spans.to_vec(),
        });
    }
    context.diagnostics.sort_by_key(|diagnostic| {
        diagnostic
            .span
            .map_or((usize::MAX, usize::MAX), |span| (span.start, span.end))
    });
    Ok(context.diagnostics)
}

struct ExecutionAuthority {
    authorized: bool,
    unauthorized: Vec<SourceSpan>,
    diagnostics: Vec<Diagnostic>,
}

impl ExecutionAuthority {
    fn selector(&mut self, entry: &MetadataEntry, key: &str) {
        if self.authorized {
            self.diagnostics.push(metadata_error(
                DiagnosticCode::UnsupportedQmdMetadata,
                format!("document `{key}` is unsupported; execution selectors belong to the collection configuration"),
                entry.span,
            ));
        } else {
            self.unauthorized.push(entry.span);
        }
    }
}

fn metadata_error(code: DiagnosticCode, message: String, span: SourceSpan) -> Diagnostic {
    Diagnostic {
        code,
        severity: Severity::Error,
        message,
        span: Some(span),
        related_spans: Vec::new(),
    }
}
