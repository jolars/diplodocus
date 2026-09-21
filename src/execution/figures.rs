//! Figure-option validation over final, validated output slots.

use crate::diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEntity, DiagnosticSource, Severity,
};
use crate::ir::OutputRepresentation;

use super::{
    CellExecutionResult, CellOutcome, ExecutionFailure, ExecutionFailureKind, ExecutionOutput,
    ExecutionPage, OptionOrigin,
};

/// Check subcaption counts after all page-level display updates and clearing.
///
/// Callers supply final output slots with validated representations in MIME
/// preference order. Each selected figure asset counts once, even if the same
/// asset appears in several slots. Alternatives, cleared slots, and skipped
/// cells do not add figures. Visibility options never suppress this validation.
/// This function does not validate asset bytes or grant rendering trust.
///
/// # Errors
///
/// Returns an output-validation failure containing one `invalid-figure-options`
/// diagnostic per mismatched cell, in input order. Diagnostics identify the
/// owning cell's winning declaration, even when another cell updated its output.
pub fn validate_figure_options(
    page: &ExecutionPage,
    cells: &[CellExecutionResult],
) -> Result<(), ExecutionFailure> {
    let diagnostics: Vec<_> = cells
        .iter()
        .filter_map(|cell| {
            let captions = &cell.options.fig_subcap;
            if matches!(cell.outcome, CellOutcome::Skipped { .. }) || captions.value.is_empty() {
                return None;
            }
            let figures = cell.outputs.iter().filter(|output| selected_figure(output)).count();
            if captions.value.len() == figures {
                return None;
            }
            let mut diagnostic = Diagnostic::new(
                DiagnosticCode::InvalidFigureOptions,
                Severity::Error,
                format!(
                    "The cell declares {} subcaption(s) but has {figures} selected figure(s) in its final output.",
                    captions.value.len(),
                ),
            )
            .with_entity(DiagnosticEntity::Content { id: page.collection.clone() })
            .with_source(DiagnosticSource::Repository {
                repository: page.source.repository.clone(),
                path: page.source.path.clone(),
            });
            let span = match captions.origin {
                OptionOrigin::Default => cell.span,
                OptionOrigin::Document { span }
                | OptionOrigin::Inline { span }
                | OptionOrigin::Hashpipe { span }
                | OptionOrigin::FenceIdentifier { span } => span,
            };
            diagnostic.span = Some(span);
            if span != cell.span {
                diagnostic.related_spans.push(cell.span);
            }
            Some(diagnostic)
        })
        .collect();
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(ExecutionFailure {
            kind: ExecutionFailureKind::OutputValidation,
            diagnostics,
            cleanup_diagnostics: Vec::new(),
        })
    }
}

fn selected_figure(output: &ExecutionOutput) -> bool {
    matches!(
        output.output.representations.first(),
        Some(OutputRepresentation::Asset { media_type, .. })
            if output.selected_mime_type.as_deref() == Some(media_type.as_str())
                && matches!(media_type.as_str(), "image/svg+xml" | "image/png" | "image/jpeg")
    )
}
